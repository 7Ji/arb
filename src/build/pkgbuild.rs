// TODO: Split this into multiple modules
use crate::{
        identity::IdentityActual,
        source::{
            self,
            git::{self, Gmr},
            MapByDomain,
        },
        roots::{
            CommonRoot,
            BaseRoot,
            OverlayRoot, BootstrappingOverlayRoot,
        },
        threading::{
            self,
            wait_if_too_busy,
        }, filesystem::remove_dir_all_try_best, build::sign::sign_pkgs
    };
use git2::Oid;
use serde::Deserialize;
use std::{
        collections::HashMap,
        ffi::OsString,
        fs::{
            create_dir_all,
            remove_dir_all,
            rename
        },
        io::{Write, Read},
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
use super::{depend::Depends, DepHashStrategy};
use super::depend::DbHandle;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum PkgbuildConfig {
    Simple (String),
    Complex {
        url: String,
        branch: Option<String>,
        subtree: Option<String>,
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
pub(super) struct PKGBUILD {
    pub(super) base: String,
    branch: String,
    build: PathBuf,
    commit: git2::Oid,
    depends: Depends,
    pub(super) extracted: bool,
    git: PathBuf,
    home_binds: Vec<String>,
    _names: Vec<String>,
    pub(super) need_build: bool,
    pkgid: String,
    pkgdir: PathBuf,
    pkgver: Pkgver,
    sources: Vec<source::Source>,
    subtree: Option<PathBuf>,
    url: String,
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

impl PKGBUILD {
    fn new(
        name: &str, url: &str, build_parent: &Path, git_parent: &Path,
        branch: Option<&str>, subtree: Option<&str>, deps: Option<&Vec<String>>,
        makedeps: Option<&Vec<String>>, home_binds: Option<&Vec<String>>
    ) -> Self
    {
        let url = if url == "AUR" {
            format!("https://aur.archlinux.org/{}.git", name)
        } else if url.starts_with("GITHUB/") {
            if url.ends_with('/') {
                format!("https://github.com/{}{}.git", &url[7..], name)
            } else {
                format!("https://github.com/{}.git", &url[7..])
            }
        } else {
            url.to_string()
        };
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
            extracted: false,
            git: git_parent.join(
                format!("{:016x}",xxh3_64(url.as_bytes()))),
            home_binds: match home_binds {
                Some(home_binds) => home_binds.clone(),
                None => vec![],
            },
            _names: vec![],
            need_build: false,
            pkgid: String::new(),
            pkgdir: PathBuf::from("pkgs"),
            pkgver: Pkgver::Plain,
            sources: vec![],
            subtree: match subtree {
                Some(subtree) => {
                    if subtree.ends_with('/') || subtree.starts_with('/') {
                        let mut subtree = subtree.to_owned();
                        if subtree.ends_with('/') {
                            subtree = format!("{}/{}", 
                                subtree.trim_end_matches('/'), name);
                        }
                        Some(PathBuf::from(subtree.trim_start_matches('/')))
                    } else {
                        Some(PathBuf::from(subtree))
                    }
                },
                None => None,
            },
            url,
        }
    }
    // If healthy, return the latest commit id
    fn healthy(&self) -> Result<Oid, ()> {
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None).or_else(|_|
        {
            eprintln!("Failed to open or init bare repo {}",
                self.git.display());
            Err(())
        })?;
        let commit = repo.get_branch_commit_or_subtree_id(
            &self.branch, self.subtree.as_deref()
        )?;
        match &self.subtree {
            Some(_) => println!("PKGBUILD '{}' at tree '{}'", 
                        self.base, commit),
            None => println!("PKGBUILD '{}' at commit '{}'", self.base, commit),
        }
        repo.get_pkgbuild_blob(&self.branch, 
                self.subtree.as_deref())
            .or_else(|_|{
                eprintln!("Failed to get PKGBUILD blob");
                Err(())
            })?;
        Ok(commit)
    }

    fn healthy_set_commit(&mut self) -> bool {
        match self.healthy() {
            Ok(commit) => {
                self.commit = commit;
                true
            },
            Err(()) => false,
        }
    }

    fn dump<P: AsRef<Path>> (&self, target: P) -> Result<(), ()> {
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None)?;
        let blob = repo.get_pkgbuild_blob(&self.branch,
            self.subtree.as_deref())?;
        let mut file =
            std::fs::File::create(target).or(Err(()))?;
        file.write_all(blob.content()).or(Err(()))
    }

    fn dep_reader_file<P: AsRef<Path>> (
        actual_identity: &IdentityActual, pkgbuild_file: P
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

    fn dep_reader<P: AsRef<Path>>(&self, actual_identity: &IdentityActual, dir: P) 
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

    pub(super) fn extractor_source(
        &self, actual_identity: &IdentityActual) -> Result<Child, ()> 
    {
        const SCRIPT: &str = include_str!("../../scripts/extract_sources.bash");
        if let Err(e) = create_dir_all(&self.build) {
            eprintln!("Failed to create build dir: {}", e);
            return Err(());
        }
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None)?;
        repo.checkout(
            &self.build, &self.branch, self.subtree.as_deref()
        )?;
        source::extract(&self.build, &self.sources);
        let pkgbuild_dir = self.build.canonicalize().or_else(
        |e|{
            eprintln!("Failed to canoicalize build dir path: {}", e);
            Err(())
        })?;
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
            Ok(child) => Ok(child),
            Err(e) => {
                eprintln!("Faiiled to spawn extractor: {}", e);
                Err(())
            },
        }
    }

    fn _extract_source(&self, actual_identity: &IdentityActual) -> Result<(), ()> {
        if self.extractor_source(actual_identity).or_else(|_|{
            eprintln!("Failed to spawn child to extract source");
            Err(())
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

    fn fill_id_dir(&mut self, dephash_strategy: &DepHashStrategy) {
        let mut pkgid = if let DepHashStrategy::None = dephash_strategy 
        {
            format!("{}-{}", self.base, self.commit)
        } else {
            format!( "{}-{}-{:016x}", self.base, self.commit, 
                self.depends.hash)
        };
        if let Pkgver::Func { pkgver } = &self.pkgver {
            pkgid.push('-');
            pkgid.push_str(&pkgver);
        }
        self.pkgdir.push(&pkgid);
        self.pkgid = pkgid;
        println!("PKGBUILD '{}' pkgid is '{}'", self.base, self.pkgid);
    }

    pub(super) fn get_temp_pkgdir(&self) -> Result<PathBuf, ()> {
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

    pub(super) fn get_build_command(
        &self,
        actual_identity: &IdentityActual,
        temp_pkgdir: &Path
    ) 
        -> Result<Command, ()> 
    {
        let cwd = actual_identity.cwd();
        let cwd_no_root = actual_identity.cwd_no_root();
        let pkgdest = cwd.join(temp_pkgdir);
        let root = OverlayRoot::get_root_no_init(&self.base);
        let mut builder = cwd.join(&root);
        builder.push(cwd_no_root);
        builder.push(&self.build);
        let chroot = cwd.join(&root);
        let mut command = Command::new("/bin/bash");
        command
            .current_dir(&builder)
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
        actual_identity.set_root_chroot_drop_command(&mut command, chroot);
        Ok(command)
    }

    pub(super) fn link_pkgs(&self) -> Result<(), ()> {
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

    pub(super) fn finish_build(&self, 
        actual_identity: &IdentityActual, temp_pkgdir: &Path, sign: Option<&str>
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
            sign_pkgs(actual_identity, temp_pkgdir, key)?;
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

    pub(super) fn _get_overlay_root(
        &self, actual_identity: &IdentityActual, nonet: bool
    ) -> Result<OverlayRoot, ()> 
    {
        OverlayRoot::_new(&self.base, actual_identity, 
            &self.depends.needs, self.get_home_binds(), nonet)
    }

    pub(super) fn get_bootstrapping_overlay_root(
        &self, actual_identity: &IdentityActual, nonet: bool
    ) -> Result<BootstrappingOverlayRoot, ()> 
    {
        BootstrappingOverlayRoot::new(&self.base, actual_identity, 
            &self.depends.needs, self.get_home_binds(), nonet)
    }
}

// struct PkgsDepends (Vec<Depends>);
pub(super) struct PKGBUILDs (pub(super) Vec<PKGBUILD>);

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
            Ok(repos_map) => repos_map,
            Err(_) => {
                eprintln!("Failed to convert to repos map");
                return Err(())
            },
        };
        git::Repo::sync_mt(repos_map, hold, proxy)
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
            if pkgbuilds.sync(hold, proxy, gmr).is_err() {
                eprintln!("Failed to sync PKGBUILDs");
                return Err(())
            }
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
        &mut self, actual_identity: &IdentityActual, dir: P, db_handle: &DbHandle,
        dephash_strategy: &DepHashStrategy
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
            match pkgbuild.depends.needed_and_hash(
                db_handle, dephash_strategy) 
            {
                Ok(_) => {
                    if let DepHashStrategy::None = dephash_strategy {
                        println!("PKGBUILD '{}' needed dependencies: {:?}", 
                                &pkgbuild.base, &pkgbuild.depends.needs);
                    } else {
                        println!("PKGBUILD '{}' dephash {:016x}, \
                                needed dependencies: {:?}", 
                                &pkgbuild.base, pkgbuild.depends.hash, 
                                &pkgbuild.depends.needs);
                    }
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

    fn check_deps<P: AsRef<Path>> (
        &mut self, actual_identity: &IdentityActual, dir: P, root: P, 
        dephash_strategy: &DepHashStrategy
    )   -> Result<Vec<String>, ()>
    {
        let db_handle = DbHandle::new(root)?;
        self.get_deps(actual_identity, dir, &db_handle, dephash_strategy)
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
        &mut self, actual_identity: &IdentityActual, dir: P
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
            match child_out.read(&mut output_buffer[..]) {
                Ok(read_this) => 
                    output.extend_from_slice(&output_buffer[0..read_this]),
                Err(e) => {
                    eprintln!("Failed to read stdout child: {}", e);
                    child.kill().expect("Failed to kill child");
                    return Err(())
                },
            }
        }
        drop(child_in);
        match child_out.read_to_end(&mut output) {
            Ok(_) => (),
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
        actual_identity: &IdentityActual, 
        pkgbuilds: &mut [&mut PKGBUILD]
    ) 
        -> Result<(), ()> 
    {
        let mut children = vec![];
        let mut bad = false;
        for pkgbuild in pkgbuilds.iter_mut() {
            if let Ok(child) = 
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
        &mut self, actual_identity: &IdentityActual, dir: P
    )
        -> Result<(), ()> 
    {
        let mut pkgbuilds = 
            self.filter_with_pkgver_func(actual_identity, dir)?;
        Self::extract_sources_many(actual_identity, &mut pkgbuilds)?;
        let children: Vec<Child> = pkgbuilds.iter().map(
        |pkgbuild| {
            println!("Executing pkgver() for '{}'...", &pkgbuild.base);
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
        }).collect();
        for (child, pkgbuild) in 
            zip(children, pkgbuilds.iter_mut()) 
        {
            let output = child.wait_with_output()
                .expect("Failed to wait for child");
            let pkgver = String::from_utf8_lossy(&output.stdout)
                .trim().to_string();
            println!("PKGBUILD '{}' pkgver is '{}'", &pkgbuild.base, &pkgver);
            pkgbuild.pkgver = Pkgver::Func { pkgver };
            pkgbuild.extracted = true
        }
        Ok(())
    }

    fn fill_all_ids_dirs(&mut self, dephash_strategy: &DepHashStrategy) {
        for pkgbuild in self.0.iter_mut() {
            pkgbuild.fill_id_dir(dephash_strategy)
        }
    }
    
    fn check_if_need_build(&mut self) 
        -> Result<u32, ()> 
    {
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
                pkgbuild.need_build = false;
                println!("Skipped already built '{}'",
                    pkgbuild.pkgdir.display());
                if pkgbuild.extracted {
                    pkgbuild.extracted = false;
                    let dir = pkgbuild.build.clone();
                    if let Err(_) = wait_if_too_busy(
                        &mut cleaners, 30, 
                        "cleaning builddir") {
                        bad = true
                    }
                    cleaners.push(thread::spawn(||
                        remove_dir_all_try_best(dir)
                        .or(Err(()))));
                }
            } else {
                pkgbuild.need_build = true;
                need_build += 1;
            }
        }
        if let Err(_) = threading::wait_remaining(
            cleaners, "cleaning builddirs") 
        {
            bad = true
        }
        if bad { Err(()) } else { Ok(need_build) }
    }

    pub(super) fn prepare_sources(
        &mut self,
        actual_identity: &IdentityActual, 
        basepkgs: &Vec<String>,
        holdgit: bool,
        skipint: bool,
        noclean: bool,
        proxy: Option<&str>,
        gmr: Option<&git::Gmr>,
        dephash_strategy: &DepHashStrategy,
    ) -> Result<Option<BaseRoot>, ()> 
    {

        let dir = tempfile::tempdir().or_else(|e| {
            eprintln!("Failed to create temp dir to dump PKGBUILDs: {}", e);
            Err(())
        })?;
        let cleaner = match 
            PathBuf::from("build").exists() 
        {
            true => Some(thread::spawn(|| remove_dir_all_try_best("build"))),
            false => None,
        };
        self.dump(&dir)?;
        let (netfile_sources, git_sources, _)
            = self.get_all_sources(&dir).ok_or(())?;
        source::cache_sources_mt(
            &netfile_sources, &git_sources, actual_identity,
            holdgit, skipint, proxy, gmr)?;
        if let Some(cleaner) = cleaner {
            cleaner.join()
                .expect("Failed to join build dir cleaner thread")
                .or_else(|_| {
                    eprintln!("Build dir cleaner thread panicked");
                    Err(())
                })?;
        }
        let cleaners = match noclean {
            true => None,
            false => Some(source::cleanup(netfile_sources, git_sources)),
        };
        self.fill_all_pkgvers(actual_identity, &dir)?;
        // Use the fresh DBs in target root
        let base_root = BaseRoot::db_only()?;
        self.check_deps(
            actual_identity, dir.as_ref(), base_root.path(),
            dephash_strategy)?;
        self.fill_all_ids_dirs(dephash_strategy);
        let need_builds = self.check_if_need_build()? > 0;
        if need_builds {
            let mut all_deps = vec![];
            for pkgbuild in self.0.iter() {
                if ! pkgbuild.need_build {
                    continue
                }
                for dep in pkgbuild.depends.needs.iter() {
                    all_deps.push(dep.clone())
                }
            }
            for pkg in basepkgs.iter() {
                all_deps.push(pkg.clone())
            }
            all_deps.sort_unstable();
            all_deps.dedup();
            Depends::cache_raw(&all_deps, base_root.db_path())?;
            base_root.finish(actual_identity, basepkgs)?;
            let db_handle = DbHandle::new(base_root.path())?;
            for pkgbuild in self.0.iter_mut() {
                if pkgbuild.need_build {
                    pkgbuild.depends.update_needed(&db_handle);
                }
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
    
    pub(super) fn clean_pkgdir(&self) {
        let mut used: Vec<String> = self.0.iter().map(
            |pkgbuild| pkgbuild.pkgid.clone()).collect();
        used.push(String::from("updated"));
        used.push(String::from("latest"));
        used.sort_unstable();
        source::remove_unused("pkgs", &used);
    }

    pub(super) fn link_pkgs(&self) {
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
    }
}