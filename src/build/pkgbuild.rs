// TODO: Split this into multiple modules
use crate::{
        identity::Identity,
        source::{
            self,
            git::{self, Gmr},
            MapByDomain,
        },
        roots::{
            CommonRoot,
            BaseRoot,
            OverlayRoot,
        },
        threading::{
            self,
            wait_if_too_busy,
        }
    };
use git2::Oid;
use rand::{self, Rng};
use serde::Deserialize;
use std::{
        collections::HashMap,
        ffi::OsString,
        fs::{
            create_dir_all,
            read_dir,
            remove_dir,
            remove_dir_all,
            remove_file,
            rename, File,
        },
        io::{Write, stdout, Read},
        os::unix::{
            fs::symlink,
            process::CommandExt
        },
        path::{
            PathBuf,
            Path,
        },
        process::{
            Child,
            Command, 
            Stdio
        },
        thread,
        iter::zip,
    };
use xxhash_rust::xxh3::xxh3_64;
use super::depend::Depends;
use super::depend::DbHandle;


#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum PkgbuildConfig {
    Simple (String),
    Complex {
        url: String,
        branch: Option<String>,
        subtree: Option<PathBuf>,
        deps: Option<Vec<String>>,
        makedeps: Option<Vec<String>>,
        home_binds: Option<Vec<String>>,
        binds: Option<HashMap<String, String>>
    },
}

#[derive(Clone)]
enum Pkgver {
    Plain,
    Func { pkgver: String },
}

#[derive(Clone)]
struct PKGBUILD {
    base: String,
    branch: String,
    build: PathBuf,
    commit: git2::Oid,
    depends: Depends,
    extract: bool,
    git: PathBuf,
    home_binds: Vec<String>,
    _names: Vec<String>,
    pkgid: String,
    pkgdir: PathBuf,
    pkgver: Pkgver,
    sources: Vec<source::Source>,
    subtree: Option<PathBuf>,
    url: String,
}

struct Builder<'a> {
    pkgbuild: &'a mut PKGBUILD,
    temp_pkgdir: PathBuf,
    command: Command,
    _root: OverlayRoot,
    tries: usize,
    child: Child,
    log_path: PathBuf
}

impl source::MapByDomain for PKGBUILD {
    fn url(&self) -> &str {
        self.url.as_str()
    }
}

impl git::ToReposMap for PKGBUILD {
    fn url(&self) -> &str {
        self.url.as_str()
    }

    fn hash_url(&self) -> u64 {
        xxh3_64(&self.url.as_bytes())
    }

    fn path(&self) -> Option<&Path> {
        Some(&self.git.as_path())
    }

    fn branch(&self) -> Option<String> {
        Some(self.branch.clone())
    }
}

// build/*/pkg being 0111 would cause remove_dir_all() to fail, in this case
// we use our own implementation
fn remove_dir_recursively<P: AsRef<Path>>(dir: P) 
    -> Result<(), std::io::Error>
{
    for entry in read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_symlink() && path.is_dir() {
            let er = 
                remove_dir_recursively(&path);
            match remove_dir(&path) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!(
                        "Failed to remove subdir '{}' recursively: {}", 
                        path.display(), e);
                    if let Err(e) = er {
                        eprintln!("Subdir failure: {}", e)
                    }
                    return Err(e);
                },
            }
        } else {
            remove_file(&path)?
        }
    }
    Ok(())
}

impl PKGBUILD {
    fn new(
        name: &str, url: &str, build_parent: &Path, git_parent: &Path,
        branch: Option<&str>, subtree: Option<&Path>, deps: Option<&Vec<String>>,
        makedeps: Option<&Vec<String>>, home_binds: Option<&Vec<String>>
    ) -> Self
    {
        Self {
            base: name.to_string(),
            branch: match branch {
                Some(branch) => branch.to_owned(),
                None => String::from("master"),
            },
            build: build_parent.join(name),
            commit: Oid::zero(),
            depends: Depends { 
                deps: match deps {
                    Some(deps) => deps.clone(),
                    None => vec![],
                }, 
                makedeps: {
                    let mut deps = match makedeps {
                        Some(deps) => deps.clone(),
                        None => vec![]
                    };
                    if name.ends_with("-git") {
                        deps.push(String::from("git"))
                    }
                    deps
                },
                needs: vec![],
                hash: 0,
            },
            extract: false,
            git: git_parent.join(
                format!("{:016x}",xxh3_64(url.as_bytes()))),
            home_binds: match home_binds {
                Some(home_binds) => home_binds.clone(),
                None => vec![],
            },
            _names: vec![],
            pkgid: String::new(),
            pkgdir: PathBuf::from("pkgs"),
            pkgver: Pkgver::Plain,
            sources: vec![],
            subtree: match subtree {
                Some(subtree) => Some(subtree.to_owned()),
                None => None,
            },
            url: url.to_owned(),
        }
    }
    // If healthy, return the latest commit id
    fn healthy(&self) -> Option<Oid> {
        let repo =
            match git::Repo::open_bare(&self.git, &self.url, None) {
                Some(repo) => repo,
                None => {
                    eprintln!("Failed to open or init bare repo {}",
                        self.git.display());
                    return None
                }
            };
        let commit = match repo.get_branch_commit_or_subtree_id(
            &self.branch, self.subtree.as_deref()
        ) {
            Some(id) => id,
            None => {
                eprintln!("Failed to get commit id for pkgbuild {}",
                            self.base);
                return None
            },
        };
        match &self.subtree {
            Some(_) => println!("PKGBUILD '{}' at tree '{}'", 
                        self.base, commit),
            None => println!("PKGBUILD '{}' at commit '{}'", self.base, commit),
        }
        let blob = repo.get_pkgbuild_blob(
            &self.branch, self.subtree.as_deref());
        match blob {
            Some(_) => Some(commit),
            None => {
                eprintln!("Failed to get PKGBUILD blob");
                None
            },
        }
    }

    fn healthy_set_commit(&mut self) -> bool {
        match self.healthy() {
            Some(commit) => {
                self.commit = commit;
                true
            },
            None => false,
        }
    }

    fn dump<P: AsRef<Path>> (&self, target: P) -> Result<(), ()> {
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None).ok_or(())?;
        let blob = repo.get_pkgbuild_blob(&self.branch,
            self.subtree.as_deref()).ok_or(())?;
        let mut file =
            std::fs::File::create(target).or(Err(()))?;
        file.write_all(blob.content()).or(Err(()))
    }

    /// Parse the PKGBUILD natively in Rust to set some value.
    /// Currently the only option possibly native to check is pkgver
    /// 
    /// To-be-fixed: fake positive on aur/usbrelay
    fn _parse(&mut self) -> Result<(), ()>{
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None).ok_or(())?;
        let blob = repo.get_pkgbuild_blob(&self.branch,
            self.subtree.as_deref()).ok_or(())?;
        let content = String::from_utf8_lossy(blob.content());
        for mut line in content.lines() {
            line = line.trim();
            if line.starts_with("function") {
                line = line.trim_start_matches("function");
                line = line.trim_start();
            }
            if ! line.starts_with("pkgver") {
                continue
            }
            line = line.trim_start_matches("pkgver");
            line = line.trim_start();
            if line.starts_with('(') {
                line = line.trim_start_matches('(');
                line = line.trim_start();
                if line.starts_with(')') {
                    // line = line.trim_start_matches(')');
                    // line = line.trim_start();
                } else {
                    continue
                }
            } else {
                continue
            }
            println!("Parse: Package '{}' has a pkgver function", self.base);
            self.pkgver = Pkgver::Func { pkgver: String::new() };
            return Ok(())
        }
        Ok(())
    }

    fn dep_reader_file<P: AsRef<Path>> (
        actual_identity: &Identity, pkgbuild_file: P
    ) -> std::io::Result<Child> 
    {
        actual_identity.set_root_drop_command(
            Command::new("/bin/bash")
                .arg("-ec")
                .arg(". \"$1\"; \
                    for dep in \"${depends[@]}\"; do \
                        echo \"d:${dep}\"; \
                    done; \
                    for dep in  \"${makedepends[@]}\"; do \
                        echo \"m:${dep}\"; \
                    done")
                .arg("Depends reader")
                .arg(pkgbuild_file.as_ref())
                .stdout(Stdio::piped()))
            .spawn()
    }

    fn dep_reader<P: AsRef<Path>>(&self, actual_identity: &Identity, dir: P) 
        -> std::io::Result<Child>
    {
        let pkgbuild_file = dir.as_ref().join(&self.base);
        Self::dep_reader_file(actual_identity, &pkgbuild_file)
    }

    fn get_sources_file<P: AsRef<Path>> (pkgbuild_file: P) 
        -> Option<Vec<source::Source>> 
    {
        source::get_sources(pkgbuild_file)
    }

    fn get_sources<P: AsRef<Path>> (&mut self, dir: P) -> Result<(), ()> {
        let pkgbuild_file = dir.as_ref().join(&self.base);
        match Self::get_sources_file(&pkgbuild_file) {
            Some(sources) => {
                self.sources = sources;
                Ok(())
            },
            None => Err(()),
        }
    }

    fn extractor_source(&self, actual_identity: &Identity) -> Option<Child> 
    {
        const SCRIPT: &str = include_str!("../../scripts/extract_sources.bash");
        if let Err(e) = create_dir_all(&self.build) {
            eprintln!("Failed to create build dir: {}", e);
            return None;
        }
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None)?;
        repo.checkout(
            &self.build, &self.branch, self.subtree.as_deref()
        ).ok()?;
        source::extract(&self.build, &self.sources);
        let pkgbuild_dir = self.build.canonicalize().ok()?;
        let mut arg0 = OsString::from("[EXTRACTOR/");
        arg0.push(&self.base);
        arg0.push("] /bin/bash");
        match actual_identity.set_root_drop_command(
            Command::new("/bin/bash")
                .arg0(&arg0)
                .arg("-ec")
                .arg(SCRIPT)
                .arg("Source extractor")
                .arg(&pkgbuild_dir))
            .spawn() 
        {
            Ok(child) => Some(child),
            Err(e) => {
                eprintln!("Faiiled to spawn extractor: {}", e);
                None
            },
        }
    }

    fn extract_source(&self, actual_identity: &Identity) -> Result<(), ()> {
        if self.extractor_source(actual_identity).ok_or_else(||{
            eprintln!("Failed to spawn child to extract source");
        })?
            .wait().or_else(|e|{
                eprintln!("Failed to wait for extractor: {}", e);
                Err(())
            })?
            .code().ok_or_else(||{
                eprintln!("Failed to get extractor return code");
            })? == 0 {
                Ok(())
            } else {
                Err(())
            }
    }

    fn fill_id_dir(&mut self) {
        let mut pkgid = format!( "{}-{}-{:016x}", 
            self.base, self.commit, self.depends.hash);
        if let Pkgver::Func { pkgver } = &self.pkgver {
            pkgid.push('-');
            pkgid.push_str(&pkgver);
        }
        self.pkgdir.push(&pkgid);
        self.pkgid = pkgid;
        println!("PKGBUILD '{}' pkgid is '{}'", self.base, self.pkgid);
    }

    fn get_temp_pkgdir(&self) -> Result<PathBuf, ()> {
        let mut temp_name = self.pkgid.clone();
        temp_name.push_str(".temp");
        let temp_pkgdir = self.pkgdir.with_file_name(temp_name);
        let _ = remove_dir_all(&temp_pkgdir);
        match create_dir_all(&temp_pkgdir) {
            Ok(_) => Ok(temp_pkgdir),
            Err(e) => {
                eprintln!("Failed to create temp pkgdir: {}", e);
                Err(())
            },
        }
    }

    fn get_build_command(
        &self,
        actual_identity: &Identity,
        root: &OverlayRoot,
        temp_pkgdir: &Path
    ) 
        -> Result<Command, ()> 
    {
        let mut pkgdest = actual_identity.cwd()?;
        pkgdest.push(temp_pkgdir);
        let mut cwd = root.builder(actual_identity)?;
        cwd.push(&self.build);
        let mut command = Command::new("/bin/bash");
        command
            .current_dir(cwd)
            .arg0(format!("[BUILDER/{}] /bin/bash", self.pkgid))
            .arg("/usr/bin/makepkg")
            .arg("--holdver")
            .arg("--nodeps")
            .arg("--noextract")
            .arg("--ignorearch")
            .arg("--nosign")
            .env("PKGDEST", &pkgdest);
        unsafe {
            command.pre_exec(||{
                if 0 <= libc::dup2(
                    libc::STDOUT_FILENO, libc::STDERR_FILENO
                ) {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        actual_identity.set_root_chroot_drop_command(&mut command, 
            root.path().canonicalize().or(Err(()))?);
        Ok(command)
    }

    fn remove_build(&mut self) -> Result<(), ()> {
        match remove_dir_all(&self.build) {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Failed to remove build folder naively: {}", e);
            },
        }
        remove_dir_recursively(&self.build).or_else(|e|{
            eprintln!("Failed to remove build folder recursively: {}", e);
            Err(())
        })?;
        remove_dir(&self.build).or_else(|e|{
            eprintln!("Failed to remove build folder itself: {}", e);
            Err(())
        })
    }

    fn sign_pkgs(actual_identity: &Identity, dir: &Path, key: &str) 
        -> Result<(), ()> 
    {
        let reader = match read_dir(dir) {
            Ok(reader) => reader,
            Err(e) => {
                eprintln!("Failed to read temp pkgdir: {}", e);
                return Err(())
            },
        };
        let mut bad = false;
        for entry in reader {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("Failed to read entry from temp pkgdir: {}", e);
                    bad = true;
                    continue
                },
            }.path();
            if entry.ends_with(".sig") {
                continue
            }
            let output = match actual_identity.set_root_drop_command(
                Command::new("/usr/bin/gpg")
                .arg("--detach-sign")
                .arg("--local-user")
                .arg(key)
                .arg(&entry))
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .output() {
                    Ok(output) => output,
                    Err(e) => {
                        eprintln!("Failed to spawn child to sign pkg: {}", e);
                        continue
                    },
                };
            if Some(0) != output.status.code() {
                eprintln!("Bad return from gpg");
                bad = true
            }
        }
        if bad { Err(()) } else { Ok(()) }
    }

    fn link_pkgs(&self) -> Result<(), ()> {
        let mut rel = PathBuf::from("..");
        rel.push(&self.pkgid);
        let updated = PathBuf::from("pkgs/updated");
        let mut bad = false;
        for entry in
            self.pkgdir.read_dir().or_else(|e|{
                eprintln!("Failed to read pkg dir: {}", e);
                Err(())
            })?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    eprintln!("Failed to read entry from pkg dir: {}", e);
                    bad = true;
                    continue
                },
            };
            let original = rel.join(entry.file_name());
            let link = updated.join(entry.file_name());
            if let Err(e) = symlink(&original, &link) {
                eprintln!("Failed to symlink '{}' => '{}': {}", 
                    link.display(), original.display(), e);
                bad = true
            }
        }
        if bad { Err(()) } else { Ok(()) }
    }

    fn build_finish(&self, 
        actual_identity: &Identity, temp_pkgdir: &Path, sign: Option<&str>
    ) 
        -> Result<(), ()> 
    {
        println!("Finishing building '{}'", &self.pkgid);
        if self.pkgdir.exists() {
            if let Err(e) = remove_dir_all(&self.pkgdir) {
                eprintln!("Failed to remove existing pkgdir: {}", e);
                return Err(())
            }
        }
        if let Some(key) = sign {
            Self::sign_pkgs(actual_identity, temp_pkgdir, key)?;
        }
        if let Err(e) = rename(&temp_pkgdir, &self.pkgdir) {
            eprintln!("Failed to rename temp pkgdir '{}' to persistent pkgdir \
                '{}': {}", temp_pkgdir.display(), self.pkgdir.display(), e);
            return Err(())
        }
        self.link_pkgs()?;
        println!("Finished building '{}'", &self.pkgid);
        Ok(())
    }

    fn get_home_binds(&self) -> Vec<String> {
        let mut binds = self.home_binds.clone();
        let mut go = false;
        let mut cargo = false;
        
        for dep in 
            self.depends.deps.iter().chain(self.depends.makedeps.iter()) 
        {
            match dep.as_str() {
                // Go-related
                "gcc-go" => go = true,
                "go" => go = true,
                // Rust/Cargo-related
                "cargo" => cargo = true,
                "rust" => cargo = true,
                "rustup" => cargo = true,
                _ => ()
            }
        }
        if go {
            binds.push(String::from("go"))
        }
        if cargo {
            binds.push(String::from(".cargo"))
        }
        binds.sort_unstable();
        binds.dedup();
        binds
    }

    fn builder(&mut self, actual_identity: &Identity, _nonet: bool) 
        -> Result<Builder, ()> 
    {
        let temp_pkgdir = self.get_temp_pkgdir()?;
        let home_binds = self.get_home_binds();
        let root = OverlayRoot::new(
            &self.base, actual_identity, 
            &self.depends.needs, home_binds)?;
        let mut command = self.get_build_command(
            actual_identity, &root, &temp_pkgdir)?;
        let mut log_name = String::from("log");
        let mut log_path = self.build.join(&log_name);
        while log_path.exists() {
            log_name.shrink_to(3);
            for char in rand::thread_rng().sample_iter(
                rand::distributions::Alphanumeric).take(7) 
            {
                log_name.push(char::from(char))
            }
            log_path = self.build.join(&log_name);
        }
        let log_file = File::create(&log_path).or_else(|e|{
            eprintln!("Failed to open log file: {}", e);
            Err(())
        })?;
        let child = command.stdout(log_file).spawn().or_else(
        |e|{
            eprintln!("Failed to spawn child: {}", e); Err(())
        })?;
        println!("Start building '{}", &self.pkgid);
        Ok(Builder {
            pkgbuild: self,
            temp_pkgdir,
            command,
            _root: root,
            tries: 0,
            child,
            log_path
        })
    }

}

fn file_to_stdout<P: AsRef<Path>>(file: P) -> Result<(), ()> {
    let file_p = file.as_ref();
    let mut file = match File::open(&file) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to open '{}': {}", file_p.display(), e);
            return Err(())
        },
    };
    let mut buffer = vec![0; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(size) => {
                if size == 0 {
                    return Ok(())
                }
                if let Err(e) = stdout().write_all(&buffer[0..size]) 
                {
                    eprintln!("Failed to write log content to stdout: {}", e);
                    return Err(())
                }
            },
            Err(e) => {
                eprintln!("Failed to read from '{}': {}", file_p.display(), e);
                return Err(())
            },
        }

    }
}

struct Builders<'a> (Vec<Builder<'a>>);

impl<'a> Builders<'a> {
    const BUILD_MAX_TRIES: usize = 3;
    fn wait_noop(&mut self, actual_identity: &Identity, sign: Option<&str>) 
        -> bool 
    {
        let mut bad = false;
        loop {
            let mut finished = None;
            for (id, builder) in 
                self.0.iter_mut().enumerate() 
            {
                match builder.child.try_wait() {
                    Ok(status) => match status {
                        Some(_) => {
                            finished = Some(id);
                            break
                        },
                        None => continue,
                    }
                    Err(e) => { // Kill bad child
                        eprintln!("Failed to wait for child: {}", e);
                        if let Err(e) = builder.child.kill() {
                            eprintln!("Failed to kill child: {}", e);
                        }
                        finished = Some(id);
                        bad = true;
                        break
                    },
                };
            }
            let mut builder = match finished {
                Some(finished) => self.0.swap_remove(finished),
                None => break, // No child waitable
            };
            println!("Log of building '{}':", &builder.pkgbuild.pkgid);
            if file_to_stdout(&builder.log_path).is_err() {
                println!("Warning: failed to read log to stdout, \
                    you could still manually check the log file '{}'",
                    builder.log_path.display())
            }
            println!("End of Log of building '{}'", &builder.pkgbuild.pkgid);
            if builder.pkgbuild.remove_build().is_err() {
                eprintln!("Failed to remove build dir");
                bad = true;
            }
            match builder.child.wait() {
                Ok(status) => {
                    match status.code() {
                        Some(code) => {
                            if code == 0 {
                                if builder.pkgbuild.build_finish(
                                    actual_identity,
                                    &builder.temp_pkgdir, sign).is_err() 
                                {
                                    eprintln!("Failed to finish build for {}",
                                        &builder.pkgbuild.base);
                                    bad = true
                                }
                                continue
                            }
                            eprintln!("Bad return from builder child: {}",
                                        code);
                        },
                        None => eprintln!("Failed to get return code from\
                                builder child"),
                    }
                },
                Err(e) => {
                    eprintln!("Failed to get child output: {}", e);
                    bad = true;
                },
            };
            if builder.tries >= Self::BUILD_MAX_TRIES {
                eprintln!("Max retries met for building {}, giving up",
                    &builder.pkgbuild.base);
                if let Err(e) = remove_dir_all(
                    &builder.temp_pkgdir
                ) {
                    eprintln!("Failed to remove temp pkg dir for failed \
                            build: {}", e);
                    bad = true
                }
                continue
            }
            if builder.pkgbuild.extract_source(actual_identity).is_err() {
                eprintln!("Failed to re-extract source to rebuild");
                bad = true;
                continue
            }
            let log_file = match File::create(&builder.log_path) {
                Ok(log_file) => log_file,
                Err(e) => {
                    eprintln!("Failed to create log file: {}", e);
                    continue
                },
            };
            builder.tries += 1;
            builder.child = match builder.command.stdout(log_file).spawn() {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("Failed to spawn child: {}", e);
                    bad = true;
                    continue
                },
            };
            self.0.push(builder)
        }
        bad
    }

}

// struct PkgsDepends (Vec<Depends>);
pub(super) struct PKGBUILDs (Vec<PKGBUILD>);

impl PKGBUILDs {
    pub(super) fn from_config(config: &HashMap<String, PkgbuildConfig>) 
        -> Result<Self, ()> 
    {
        let build_parent = PathBuf::from("build");
        let git_parent = PathBuf::from("sources/PKGBUILD");
        let mut pkgbuilds: Vec<_> = config.iter().map(|
            (name, detail)|
        {
            match detail {
                PkgbuildConfig::Simple(url) => PKGBUILD::new(
                    name, url, &build_parent, &git_parent, 
                    None, None, None, None, 
                    None
                ),
                PkgbuildConfig::Complex { url, branch,
                    subtree, deps, 
                    makedeps,
                    home_binds,binds: _ 
                } => PKGBUILD::new(
                    name, url, &build_parent, &git_parent,
                    branch.as_deref(), subtree.as_deref(), 
                    deps.as_ref(), makedeps.as_ref(), home_binds.as_ref())
            }
        }).collect();
        pkgbuilds.sort_unstable_by(
            |a, b| a.base.cmp(&b.base));
        Ok(Self(pkgbuilds))
    }

    fn sync(&self, hold: bool, proxy: Option<&str>, gmr: Option<&Gmr>) 
        -> Result<(), ()> 
    {
        let map =
            PKGBUILD::map_by_domain(&self.0);
        let repos_map =
            match git::ToReposMap::to_repos_map(
                map, "sources/PKGBUILD", gmr) 
        {
            Some(repos_map) => repos_map,
            None => {
                eprintln!("Failed to convert to repos map");
                return Err(())
            },
        };
        git::Repo::sync_mt(repos_map, hold, proxy)
    }

    fn _healthy(&self) -> bool {
        for pkgbuild in self.0.iter() {
            if pkgbuild.healthy().is_none() {
                return false
            }
        }
        true
    }

    fn healthy_set_commit(&mut self) -> bool {
        for pkgbuild in self.0.iter_mut() {
            if ! pkgbuild.healthy_set_commit() {
                return false
            }
        }
        true
    }


    pub(super) fn from_config_healthy(
        config: &HashMap<String, PkgbuildConfig>, 
        hold: bool, noclean: bool, proxy: Option<&str>, gmr: Option<&Gmr>
    ) -> Result<Self, ()>
    {
        let mut pkgbuilds = Self::from_config(config)?;
        let update_pkg = if hold {
            if pkgbuilds.healthy_set_commit(){
                println!(
                    "Holdpkg set and all PKGBUILDs healthy, no need to update");
                false
            } else {
                eprintln!("Warning: holdpkg set, but PKGBUILDs unhealthy, \
                           need update");
                true
            }
        } else {
            true
        };
        // Should not need sort, as it's done when pkgbuilds was read
        let mut used: Vec<String> = pkgbuilds.0.iter().map(|pkgbuild|
            format!("{:016x}", xxh3_64(pkgbuild.url.as_bytes()))).collect();
        used.sort_unstable();
        used.dedup();
        let cleaner = match noclean {
            true => None,
            false => Some(thread::spawn(move || 
                        source::remove_unused("sources/PKGBUILD", &used))),
        };
        if update_pkg {
            pkgbuilds.sync(hold, proxy, gmr)?;
            if ! pkgbuilds.healthy_set_commit() {
                eprintln!("Updating broke some of our PKGBUILDs");
                return Err(())
            }
        }
        if let Some(cleaner) = cleaner {
            cleaner.join()
                .expect("Failed to join PKGBUILDs cleaner thread");
        }
        Ok(pkgbuilds)
    }

    fn dump<P: AsRef<Path>> (&self, dir: P) -> Result<(), ()> {
        let dir = dir.as_ref();
        let mut bad = false;
        for pkgbuild in self.0.iter() {
            let target = dir.join(&pkgbuild.base);
            if pkgbuild.dump(&target).is_err() {
                eprintln!("Failed to dump PKGBUILD '{}' to '{}'",
                    pkgbuild.base, target.display());
                bad = true
            }
        }
        if bad { Err(()) } else { Ok(()) }
    }

    fn get_deps<P: AsRef<Path>> (
        &mut self, actual_identity: &Identity, dir: P, db_handle: &DbHandle
    ) -> Result<Vec<String>, ()>
    {
        let mut bad = false;
        let mut children = vec![];
        for pkgbuild in self.0.iter() {
            match pkgbuild.dep_reader(actual_identity, &dir) {
                Ok(child) => children.push(child),
                Err(e) => {
                    eprintln!(
                        "Failed to spawn dep reader for PKGBUILD '{}': {}",
                        pkgbuild.base, e);
                    bad = true
                },
            }
        }
        if bad {
            for mut child in children {
                if let Err(e) = child.kill() {
                    eprintln!("Failed to kill child: {}", e)
                }
            }
            return Err(())
        }
        assert!(self.0.len() == children.len());
        let mut all_deps = vec![];
        for (pkgbuild, child) in 
            zip(self.0.iter_mut(), children) 
        {
            let output = child.wait_with_output()
                .expect("Failed to wait for child");
            for line in 
                output.stdout.split(|byte| byte == &b'\n') 
            {
                if line.len() == 0 {
                    continue;
                }
                let dep = 
                    String::from_utf8_lossy(&line[2..]).into_owned();
                match &line[0..2] {
                    b"d:" => pkgbuild.depends.deps.push(dep),
                    b"m:" => pkgbuild.depends.makedeps.push(dep),
                    _ => ()
                }
            }
            pkgbuild.depends.deps.sort_unstable();
            pkgbuild.depends.makedeps.sort_unstable();
            pkgbuild.depends.deps.dedup();
            pkgbuild.depends.makedeps.dedup();
            match pkgbuild.depends.needed_and_hash(db_handle) {
                Ok(_) => {
                    println!("PKGBUILD '{}' dephash {:016x}, \
                            needed dependencies: {:?}", 
                            &pkgbuild.base, pkgbuild.depends.hash, 
                            &pkgbuild.depends.needs);
                    for need in pkgbuild.depends.needs.iter() {
                        all_deps.push(need.clone())
                    }
                },
                Err(_) => {
                    eprintln!("Failed to get needed deps for package '{}'",
                            &pkgbuild.base);
                    bad = true
                },
            }
        }
        if bad {
            return Err(())
        }
        all_deps.sort_unstable();
        all_deps.dedup();
        Ok(all_deps)
    }

    fn check_deps<P: AsRef<Path>, S: AsRef<str>> (
        &mut self, actual_identity: &Identity, dir: P, root: S
    )   -> Result<Vec<String>, ()>
    {
        let db_handle = DbHandle::new(&root)?;
        self.get_deps(actual_identity, dir, &db_handle)
    }

    fn get_all_sources<P: AsRef<Path>> (&mut self, dir: P)
      -> Option<(Vec<source::Source>, Vec<source::Source>, Vec<source::Source>)>
    {
        let mut sources_non_unique = vec![];
        let mut bad = false;
        for pkgbuild in self.0.iter_mut() {
            if pkgbuild.get_sources(&dir).is_err() {
                eprintln!("Failed to get sources for PKGBUILD '{}'", 
                    pkgbuild.base);
                bad = true
            } else {
                for source in pkgbuild.sources.iter() {
                    sources_non_unique.push(source);
                }
            }
        }
        if bad {
            None
        } else {
            source::unique_sources(&sources_non_unique)
        }
    }

    fn filter_with_pkgver_func<P: AsRef<Path>>(
        &mut self, actual_identity: &Identity, dir: P
    ) -> Result<Vec<&mut PKGBUILD>, ()> 
    {
        let mut buffer = vec![];
        for pkgbuild in self.0.iter() {
            for byte in pkgbuild.base.bytes() {
                buffer.push(byte)
            }
            buffer.push(b'\n');
        }
        let mut child = match actual_identity.set_root_drop_command(
            Command::new("/bin/bash")
                .arg("-c")
                .arg(
                   "cd \"$1\"
                    while read -r line; do \
                        source \"$line\"; \
                        type -t pkgver; \
                        printf '|'; \
                        unset -f pkgver; \
                    done")
                .arg("Type Identifier")
                .arg(dir.as_ref())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()))
                .stderr(Stdio::null())
            .spawn() 
        {
            Ok(child) => child,
            Err(e) => {
                eprintln!("Failed to spawn child to read pkgver types: {}", e);
                return Err(())
            },
        };
        let mut child_in = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                eprintln!("Failed to open stdin");
                child.kill().expect("Failed to kill child");
                return Err(())
            },
        };
        let mut child_out = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                eprintln!("Failed to open stdin");
                child.kill().expect("Failed to kill child");
                return Err(())
            },
        };
        let mut output = vec![];
        let mut output_buffer = vec![0; libc::PIPE_BUF];
        let mut written = 0;
        let total = buffer.len();
        while written < total {
            let mut end = written + libc::PIPE_BUF;
            if end > total {
                end = total;
            }
            match child_in.write(&buffer[written..end]) {
                Ok(written_this) => written += written_this,
                Err(e) => {
                    eprintln!("Failed to write buffer to child: {}", e);
                    child.kill().expect("Failed to kill child");
                    return Err(())
                },
            }
            match child_out.read(&mut output_buffer) {
                Ok(read_this) => 
                    output.extend_from_slice(&output_buffer[0..read_this]),
                Err(e) => {
                    eprintln!("Failed to read stdout child: {}", e);
                    child.kill().expect("Failed to kill child");
                    return Err(())
                },
            }
        }
        if let Err(e) = child_in.flush() {
            eprintln!("Failed to flush child stdin: {}", e);
            return Err(())
        }
        drop(child_in);
        match child_out.read_to_end(&mut output_buffer) {
            Ok(_) => output.append(&mut output_buffer),
            Err(e) => {
                eprintln!("Failed to read stdout child: {}", e);
                child.kill().expect("Failed to kill child");
                return Err(())
            },
        }
        if child
            .wait()
            .or_else(|e|{
                eprintln!(
                    "Failed to wait for child reading pkgver type: {}", e);
                Err(())
            })?
            .code()
            .ok_or_else(||{
                eprintln!("Failed to get return code from child reading \
                        pkgver type")
            })? != 0 {
                eprintln!("Reader bad return");
                return Err(())
            }
        let types: Vec<&[u8]> = 
            output.split(|byte| *byte == b'|').collect();
        let types = &types[0..self.0.len()];
        assert!(types.len() == self.0.len());
        let mut pkgbuilds_with_pkgver_func = vec![];
        for (pkgbuild, pkgver_type) in 
            zip(self.0.iter_mut(), types.iter()) 
        {
            if pkgver_type == b"function\n" {
                pkgbuilds_with_pkgver_func.push(pkgbuild)
            }
        }
        Ok(pkgbuilds_with_pkgver_func)
    }

    fn extract_sources_many(
        actual_identity: &Identity, 
        pkgbuilds: &mut [&mut PKGBUILD]
    ) 
        -> Result<(), ()> 
    {
        let mut children = vec![];
        let mut bad = false;
        for pkgbuild in pkgbuilds.iter_mut() {
            if let Some(child) = 
                pkgbuild.extractor_source(actual_identity)
            {
                children.push(child);
            } else {
                bad = true;
            }
        }
        for mut child in children {
            child.wait().expect("Failed to wait for child");
        }
        if bad { Err(()) } else { Ok(()) }
    }

    fn fill_all_pkgvers<P: AsRef<Path>>(
        &mut self, actual_identity: &Identity, dir: P
    )
        -> Result<(), ()> 
    {
        let mut pkgbuilds = 
            self.filter_with_pkgver_func(actual_identity, dir)?;
        let _ = remove_dir_recursively("build");
        Self::extract_sources_many(actual_identity, &mut pkgbuilds)?;
        let children: Vec<Child> = pkgbuilds.iter().map(
        |pkgbuild|
            actual_identity.set_root_drop_command(
                Command::new("/bin/bash")
                    .arg("-ec")
                    .arg("srcdir=\"$1\"; cd \"$1\"; source ../PKGBUILD; pkgver")
                    .arg("Pkgver runner")
                    .arg(pkgbuild.build.join("src")
                        .canonicalize()
                        .expect("Failed to canonicalize dir"))
                    .stdout(Stdio::piped()))
                .spawn()
                .expect("Failed to run script")
        ).collect();
        for (child, pkgbuild) in 
            zip(children, pkgbuilds.iter_mut()) 
        {
            let output = child.wait_with_output()
                .expect("Failed to wait for child");
            pkgbuild.pkgver = Pkgver::Func { pkgver:
                String::from_utf8_lossy(&output.stdout).trim().to_string()};
            pkgbuild.extract = true
        }
        Ok(())
    }

    fn fill_all_ids_dirs(&mut self) {
        for pkgbuild in self.0.iter_mut() {
            pkgbuild.fill_id_dir()
        }
    }
    
    fn extract_if_need_build(&mut self, actual_identity: &Identity) 
        -> Result<u32, ()> 
    {
        let mut pkgbuilds_need_build = vec![];
        let mut cleaners = vec![];
        let mut bad = false;
        let mut need_build = 0;
        for pkgbuild in self.0.iter_mut() {
            let mut built = false;
            if let Ok(mut dir) = pkgbuild.pkgdir.read_dir() {
                if let Some(_) = dir.next() {
                    built = true;
                }
            }
            if built { // Does not need build
                println!("Skipped already built '{}'",
                    pkgbuild.pkgdir.display());
                if pkgbuild.extract {
                    let dir = pkgbuild.build.clone();
                    if let Err(_) = wait_if_too_busy(
                        &mut cleaners, 30, 
                        "cleaning builddir") {
                        bad = true
                    }
                    cleaners.push(thread::spawn(||
                        remove_dir_recursively(dir)
                        .or(Err(()))));
                    pkgbuild.extract = false;
                }
            } else {
                if ! pkgbuild.extract {
                    pkgbuild.extract = true;
                    pkgbuilds_need_build.push(pkgbuild);
                }
                need_build += 1;
            }
        }
        if let Err(_) = Self::extract_sources_many(actual_identity, 
            &mut pkgbuilds_need_build) 
        {
            bad = true
        }
        if let Err(_) = threading::wait_remaining(
            cleaners, "cleaning builddirs") 
        {
            bad = true
        }
        if bad { Err(()) } else { Ok(need_build) }
    }

    fn remove_builddir() -> Result<(), std::io::Error> {
        // Go the simple way first
        match remove_dir_all("build") {
            Ok(_) => return Ok(()),
            Err(e) => eprintln!("Failed to clean: {}", e),
        }
        // build/*/pkg being 0111 would cause remove_dir_all() to fail, in this case
        // we use our only implementation
        remove_dir_recursively("build")?;
        remove_dir("build")
    }

    pub(super) fn prepare_sources<P: AsRef<Path>>(
        &mut self,
        actual_identity: &Identity, 
        basepkgs: Option<&Vec<String>>,
        dir: P,
        holdgit: bool,
        skipint: bool,
        noclean: bool,
        proxy: Option<&str>,
        gmr: Option<&git::Gmr>
    ) -> Result<Option<BaseRoot>, ()> 
    {
        let cleaner = match 
            PathBuf::from("build").exists() 
        {
            true => Some(thread::spawn(|| Self::remove_builddir())),
            false => None,
        };
        self.dump(&dir)?;
        let (netfile_sources, git_sources, _)
            = self.get_all_sources(&dir).ok_or(())?;
        source::cache_sources_mt(
            &netfile_sources, &git_sources, actual_identity,
            holdgit, skipint, proxy, gmr)?;
        if let Some(cleaner) = cleaner {
            match cleaner.join()
                .expect("Failed to join build dir cleaner thread") {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Failed to clean build dir: {}", e);
                    return Err(())
                },
            }
        }
        let cleaners = match noclean {
            true => None,
            false => Some(source::cleanup(netfile_sources, git_sources)),
        };
        self.fill_all_pkgvers(actual_identity, &dir)?;
        // Use the fresh DBs in target root
        let base_root = BaseRoot::db_only()?;
        let all_deps = self.check_deps(
            actual_identity, dir.as_ref(), base_root.as_str())?;
        self.fill_all_ids_dirs();
        let need_builds = self.extract_if_need_build(actual_identity)? > 0;
        if need_builds {
            Depends::cache_raw(&all_deps, base_root.as_str())?;
            if let Some(basepkgs) = basepkgs {
                base_root.finish(actual_identity, basepkgs)?;
            } else {
                base_root.finish(actual_identity, &["base-devel"])?;
            }
        }
        if let Some(cleaners) = cleaners {
            for cleaner in cleaners {
                cleaner.join()
                .expect("Failed to join sources cleaner thread");
            }
        }
        if need_builds {
            Ok(Some(base_root))
        } else {
            Ok(None)
        }
    }

    pub(super) fn build_any_needed(
        &mut self, actual_identity: &Identity, nonet: bool, sign: Option<&str>
    ) 
        -> Result<(), ()>
    {
        let _ = remove_dir_all("pkgs/updated");
        let _ = remove_dir_all("pkgs/latest");
        if let Err(e) = create_dir_all("pkgs/updated") {
            eprintln!("Failed to create pkgs/updated: {}", e);
            return Err(())
        }
        if let Err(e) = create_dir_all("pkgs/latest") {
            eprintln!("Failed to create pkgs/latest: {}", e);
            return Err(())
        }
        let mut bad = false;
        let cpuinfo = procfs::CpuInfo::new().or_else(|e|{
            eprintln!("Failed to get cpuinfo: {}", e);
            Err(())
        })?;
        let cores = cpuinfo.num_cores();
        let mut builders = Builders(vec![]);
        for pkgbuild in self.0.iter_mut() {
            if ! pkgbuild.extract {
                continue
            }
            loop { // Wait for CPU resource
                if builders.wait_noop(actual_identity, sign) {
                    bad = true
                }
                let heavy_load = match procfs::LoadAverage::new() {
                    Ok(load_avg) => load_avg.one >= cores as f32,
                    Err(e) => {
                        eprintln!("Failed to get load avg: {}", e);
                        true
                    },
                };
                if builders.0.len() < 4 && !heavy_load {
                    break
                } else {
                    std::thread::sleep(
                        std::time::Duration::from_millis(100))
                }
            }
            let builder = match 
                pkgbuild.builder(actual_identity, nonet) 
            {
                Ok(builder) => builder,
                Err(_) => {
                    bad = true;
                    continue
                },
            };
            builders.0.push(builder)
        }
        while builders.0.len() > 0 {
            if builders.wait_noop(actual_identity, sign) {
                bad = true
            }
            std::thread::sleep(std::time::Duration::from_millis(100))
        }
        let thread_cleaner =
            thread::spawn(|| remove_dir_recursively("build"));
        let rel = PathBuf::from("..");
        let latest = PathBuf::from("pkgs/latest");
        for pkgbuild in self.0.iter() {
            if ! pkgbuild.pkgdir.exists() {
                continue;
            }
            let dirent = match pkgbuild.pkgdir.read_dir() {
                Ok(dirent) => dirent,
                Err(e) => {
                    eprintln!("Failed to read dir '{}': {}", 
                        pkgbuild.pkgdir.display(), e);
                    continue
                },
            };
            let rel = rel.join(&pkgbuild.pkgid);
            for entry in dirent {
                if let Ok(entry) = entry {
                    let original = rel.join(entry.file_name());
                    let link = latest.join(entry.file_name());
                    println!("Linking '{}' => '{}'", 
                            link.display(), original.display());
                    if let Err(e) = symlink(original, link) {
                        eprintln!("Failed to link: {}", e);
                    }
                }
            }
        }
        let _ = thread_cleaner.join()
            .expect("Failed to join cleaner thread");
        if bad { Err(()) } else { Ok(()) }
    }
    
    pub(super) fn clean_pkgdir(&self) {
        let mut used: Vec<String> = self.0.iter().map(
            |pkgbuild| pkgbuild.pkgid.clone()).collect();
        used.push(String::from("updated"));
        used.push(String::from("latest"));
        used.sort_unstable();
        source::remove_unused("pkgs", &used);
    }
}