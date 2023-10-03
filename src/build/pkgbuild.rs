// TODO: Split this into multiple modules
// TODO: Use libalpm to handle deps
// TODO: Use clean chroots to build to avoid tainting host
use crate::{
        identity::Identity,
        source::{
            self,
            git,
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
        },
    };
use git2::Oid;
use std::{
        collections::HashMap,
        ffi::OsString,
        fs::{
            create_dir_all,
            read_dir,
            remove_dir,
            remove_dir_all,
            remove_file,
            rename,
        },
        io::Write,
        os::unix::{
            fs::symlink,
            process::CommandExt, prelude::OsStrExt
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

#[derive(Clone)]
enum Pkgver {
    Plain,
    Func { pkgver: String },
}

#[derive(Clone)]
struct PKGBUILD {
    base: String,
    build: PathBuf,
    commit: git2::Oid,
    depends: Vec<String>,
    dephash: u64,
    extract: bool,
    git: PathBuf,
    names: Vec<String>,
    pkgid: String,
    pkgdir: PathBuf,
    pkgver: Pkgver,
    sources: Vec<source::Source>,
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
    fn new<S: AsRef<str>, P: AsRef<Path>>(
        name: S, url: S, build_parent: P, git_parent: P
    ) -> Self 
    {
        Self {
            base: name.as_ref().to_string(),
            build: build_parent.as_ref().join(name.as_ref()),
            commit: Oid::zero(),
            depends: vec![],
            dephash: 0,
            extract: false,
            git: git_parent.as_ref().join(name.as_ref()),
            names: vec![],
            pkgid: String::new(),
            pkgdir: PathBuf::from("pkgs"),
            pkgver: Pkgver::Plain,
            sources: vec![],
            url: url.as_ref().to_string(),
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
        let commit = match repo.get_branch_commit_id("master") {
            Some(id) => id,
            None => {
                eprintln!("Failed to get commit id for pkgbuild {}",
                            self.base);
                return None
            },
        };
        println!("PKGBUILD '{}' at commit '{}'", self.base, commit);
        let blob = repo.get_pkgbuild_blob();
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
        let blob = repo.get_pkgbuild_blob().ok_or(())?;
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
        let blob = repo.get_pkgbuild_blob().ok_or(())?;
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
                    line = line.trim_start_matches(')');
                    line = line.trim_start();
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
        actual_identity.set_command(
            Command::new("/bin/bash")
                .arg("-ec")
                .arg(". \"$1\"; \
                    for dep in \"${depends[@]}\" \"${makedepends[@]}\"; do \
                        echo \"${dep}\"; \
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

    fn pkgver_type_reader_file<P: AsRef<Path>> (
        actual_identity: &Identity, pkgbuild_file: P
    ) -> std::io::Result<Child> 
    {
        // let content = std::fs::read_to_string(pkgbuild_file);

        actual_identity.set_command(
            Command::new("/bin/bash")
                .arg("-c")
                .arg(". \"$1\"; type -t pkgver")
                .arg("Type Identifier")
                .arg(pkgbuild_file.as_ref())
                .stdout(Stdio::piped()))
            .spawn()
    }

    fn pkgver_type_reader<P: AsRef<Path>> (
        &self, actual_identity: &Identity, dir: P)
        -> std::io::Result<Child> 
    {
        let pkgbuild_file = dir.as_ref().join(&self.base);
        Self::pkgver_type_reader_file(actual_identity, &pkgbuild_file)
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
        repo.checkout_branch(&self.build, "master").ok()?;
        source::extract(&self.build, &self.sources);
        let pkgbuild_dir = self.build.canonicalize().ok()?;
        let mut arg0 = OsString::from("[EXTRACTOR/");
        arg0.push(&self.base);
        arg0.push("] /bin/bash");
        match actual_identity.set_command(
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

    fn fill_id_dir(&mut self) {
        let mut pkgid = format!( "{}-{}-{:016x}", 
            self.base, self.commit, self.dephash);
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
        nonet: bool,
        temp_pkgdir: &Path
    ) 
        -> Result<Command, ()> 
    {
        let mut command = if nonet {
            let mut command = Command::new("/usr/bin/unshare");
            command.arg("--map-root-user")
                .arg("--net")
                .arg("--")
                .arg("sh")
                .arg("-c")
                .arg(format!(
                    "ip link set dev lo up
                    unshare --map-users={}:0:1 --map-groups={}:0:1 -- \
                        makepkg --holdver --nodeps --noextract --ignorearch", 
                    unsafe {libc::getuid()}, unsafe {libc::getgid()}));
            command
        } else {
            let mut command = Command::new("/bin/bash");
            command.arg("/usr/bin/makepkg")
                .arg("--holdver")
                .arg("--nodeps")
                .arg("--noextract")
                .arg("--ignorearch");
            command
        };
        let mut pkgdest = actual_identity.home()?;
        pkgdest.push("builder");
        pkgdest.push(temp_pkgdir);
        actual_identity.set_chroot_drop_command(&mut command, 
            root.path().canonicalize().or(Err(()))?)
            .current_dir(root.builder(actual_identity)?.join(&self.build))
            .arg0(format!("[BUILDER/{}] /bin/bash", self.pkgid))
            .env("PKGDEST", &pkgdest);
        Ok(command)
    }

    fn build_try(
        &mut self,
        actual_identity: &Identity, 
        command: &mut Command, 
        temp_pkgdir: &Path
    )
        -> Result<(), ()>
    {
        const BUILD_MAX_TRIES: u8 = 3;
        for i in 0..BUILD_MAX_TRIES {
            if ! self.extract {
                let mut child = match 
                    self.extractor_source(actual_identity) 
                {
                    Some(child) => child,
                    None => return Err(()),
                };
                if let Err(e) = child.wait() {
                    eprintln!("Failed to re-extract source for '{}': {}",
                            self.pkgid, e);
                    return Err(())
                }
                self.extract = true
            }
            println!("Building '{}', try {}/{}", 
                    &self.pkgid, i + 1 , BUILD_MAX_TRIES);
            let exit_status = command
                .spawn()
                .or_else(|e|{
                    eprintln!("Failed to spawn child with command {:?}: {}",
                        command, e);
                    Err(())
                })?
                .wait()
                .or_else(|e|{
                    eprintln!("Failed to wait child with command {:?}: {}",
                        command, e);
                    Err(())
                })?;
            if let Err(e) = remove_dir_recursively(&self.build) 
            {
                eprintln!("Failed to remove build dir '{}': {}",
                            self.build.display(), e);
                return Err(())
            }
            self.extract = false;
            match exit_status.code() {
                Some(0) => {
                    println!("Successfully built to '{}'", 
                        temp_pkgdir.display());
                    return Ok(())
                },
                _ => {
                    eprintln!("Failed to build to '{}'", temp_pkgdir.display());
                    if let Err(e) = remove_dir_all(&temp_pkgdir) {
                        eprintln!("Failed to remove temp pkgdir '{}': {}", 
                                    temp_pkgdir.display(), e);
                        return Err(())
                    }
                }
            }
        }
        eprintln!("Failed to build '{}' after all tries", 
                    temp_pkgdir.display());
        Err(())
    }

    fn build_finish(&self, temp_pkgdir: &Path) -> Result<(), ()> {
        println!("Finishing building '{}'", &self.pkgid);
        if self.pkgdir.exists() {
            if let Err(e) = remove_dir_all(&self.pkgdir) {
                eprintln!("Failed to remove existing pkgdir: {}", e);
                return Err(())
            }
        }
        if let Err(e) = rename(&temp_pkgdir, &self.pkgdir) {
            eprintln!("Failed to rename temp pkgdir '{}' to persistent pkgdir \
                '{}': {}", temp_pkgdir.display(), self.pkgdir.display(), e);
            return Err(())
        }
        let mut rel = PathBuf::from("..");
        rel.push(&self.pkgid);
        let updated = PathBuf::from("pkgs/updated");
        for entry in
            self.pkgdir.read_dir().expect("Failed to read dir")
        {
            if let Ok(entry) = entry {
                let original = rel.join(entry.file_name());
                let link = updated.join(entry.file_name());
                let _ = symlink(original, link);
            }
        }
        println!("Finished building '{}'", &self.pkgid);
        Ok(())
    }

    fn build(&mut self, actual_identity: &Identity, nonet: bool) 
        -> Result<(), ()> 
    {
        let temp_pkgdir = self.get_temp_pkgdir()?;
        let root = OverlayRoot::new(
            &self.base, actual_identity, &self.depends)?;
        let mut command = self.get_build_command(
            actual_identity, &root, nonet, &temp_pkgdir)?;
        self.build_try(actual_identity, &mut command, &temp_pkgdir)?;
        self.build_finish(&temp_pkgdir)
    }

    fn get_deps_file<P: AsRef<Path>> (
        actual_identity: &Identity, pkgbuild_file: P
    ) -> std::io::Result<Depends> 
    {
        let child = match 
            Self::dep_reader_file(actual_identity, &pkgbuild_file) {
                Ok(child) => child,
                Err(e) => {
                    eprintln!(
                        "Failed to spawn child to read deps from '{}': {}",
                        pkgbuild_file.as_ref().display(), e);
                    return Err(e)
                },
            };
        let output = match child.wait_with_output() {
            Ok(output) => output,
            Err(e) => {
                eprintln!("Failed to wait for child to read dep");
                return Err(e)
            },
        };
        let mut pkg_deps = Depends(vec![]);
        for line in 
            output.stdout.split(|byte| byte == &b'\n') 
        {
            if line.len() == 0 {
                continue;
            }
            let dep = String::from_utf8_lossy(line).into_owned();
            pkg_deps.0.push(dep);
        }
        pkg_deps.0.sort();
        pkg_deps.0.dedup();
        Ok(pkg_deps)
    }

}

// struct PkgsDepends (Vec<Depends>);
pub(super) struct PKGBUILDs (Vec<PKGBUILD>);

impl PKGBUILDs {
    pub(super) fn from_yaml_config<P: AsRef<Path>>(yaml: P) -> Option<Self> {
        let f = match std::fs::File::open(&yaml) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open PKGBUILDs YAML config '{}': {}",
                    yaml.as_ref().display(), e);
                return None
            },
        };
        let config: HashMap<String, String> = 
            match serde_yaml::from_reader(f) 
        {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to parse PKGBUILDs YAML config '{}' : {}",
                    yaml.as_ref().display(), e);
                return None;
            },
        };
        let build_parent = PathBuf::from("build");
        let git_parent = PathBuf::from("sources/PKGBUILD");
        let mut pkgbuilds: Vec<PKGBUILD> = config.iter().map(
            |(name, url)| 
            PKGBUILD::new(name, url, &build_parent, &git_parent)
        ).collect();
        pkgbuilds.sort_unstable_by(
            |a, b| a.base.cmp(&b.base));
        Some(Self(pkgbuilds))
    }

    fn sync(&self, hold: bool, proxy: Option<&str>) -> Result<(), ()> 
    {
        let map =
            PKGBUILD::map_by_domain(&self.0);
        let repos_map =
            match git::ToReposMap::to_repos_map(
                map, "sources/PKGBUILD", None) 
        {
            Some(repos_map) => repos_map,
            None => {
                eprintln!("Failed to convert to repos map");
                return Err(())
            },
        };
        git::Repo::sync_mt(repos_map, git::Refspecs::MasterOnly, hold, proxy)
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


    pub(super) fn from_yaml_config_healthy<P:AsRef<Path>>(
        yaml: P, hold: bool, noclean: bool, proxy: Option<&str>
    ) -> Option<Self>
    {
        let mut pkgbuilds = Self::from_yaml_config(yaml)?;
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
        let used: Vec<String> = pkgbuilds.0.iter().map(
            |pkgbuild| pkgbuild.base.clone()).collect();
        let cleaner = match noclean {
            true => None,
            false => Some(thread::spawn(move || 
                        source::remove_unused("sources/PKGBUILD", &used))),
        };
        if update_pkg {
            pkgbuilds.sync(hold, proxy).ok()?;
            if ! pkgbuilds.healthy_set_commit() {
                eprintln!("Updating broke some of our PKGBUILDs");
                return None
            }
        }
        if let Some(cleaner) = cleaner {
            cleaner.join()
                .expect("Failed to join PKGBUILDs cleaner thread");
        }
        Some(pkgbuilds)
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
    ) -> Result<Depends, ()>
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
        let mut all_deps = Depends(vec![]);
        for (pkgbuild, child) in 
            zip(self.0.iter_mut(), children) 
        {
            let output = child.wait_with_output()
                .expect("Failed to wait for child");
            pkgbuild.depends.clear();
            let mut pkg_deps = Depends(vec![]);
            for line in 
                output.stdout.split(|byte| byte == &b'\n') 
            {
                if line.len() == 0 {
                    continue;
                }
                pkg_deps.0.push(String::from_utf8_lossy(line).into_owned());
            }
            pkg_deps.0.sort_unstable();
            pkg_deps.0.dedup();
            match pkg_deps.needed_and_hash(db_handle) {
                Ok((needed, hash)) => {
                    println!("PKGBUILD '{}' dephash {:016x}, \
                            needed dependencies: {:?}", 
                            &pkgbuild.base, hash, &needed);
                    for need in needed.iter() {
                        all_deps.0.push(need.clone())
                    }
                    pkgbuild.depends = needed;
                    pkgbuild.dephash = hash;
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
        all_deps.0.sort_unstable();
        all_deps.0.dedup();
        Ok(all_deps)
    }

    fn check_deps<P: AsRef<Path>, S: AsRef<str>> (
        &mut self, actual_identity: &Identity, dir: P, root: S
    )   -> Result<(), ()>
    {
        let db_handle = DbHandle::new(&root)?;
        let all_deps = 
            self.get_deps(actual_identity, dir, &db_handle)?;
        all_deps.cache(&root)
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
    ) -> Option<Vec<&mut PKGBUILD>> 
    {
        let mut buffer = vec![];
        for pkgbuild in self.0.iter() {
            for byte in pkgbuild.base.bytes() {
                buffer.push(byte)
            }
            buffer.push(b'\n');
        }
        let mut child = match actual_identity.set_command(
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
                eprintln!("Failed to spawn child to read pkgver types");
                return None
            },
        };
        let mut stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                eprintln!("Failed to open stdin");
                child.kill().expect("Failed to kill child");
                return None
            },
        };
        match stdin.write_all(&buffer) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to write to child stdin: {}", e);
                child.kill().expect("Failed to kill child");
                return None
            },
        }
        drop(stdin);
        let output = match child.wait_with_output() {
            Ok(output) => output,
            Err(e) => {
                eprintln!("Failed to wait for spawned script: {}", e);
                return None
            },
        };
        let mut types: Vec<&[u8]> = 
            output.stdout.split(|byte| *byte == b'|').collect();
        types.pop().expect("Failed to remove last element in types");
        assert!(types.len() == self.0.len());
        let mut pkgbuilds_with_pkgver_func = vec![];
        for (pkgbuild, pkgver_type) in 
            zip(self.0.iter_mut(), types.iter()) 
        {
            if pkgver_type == b"function\n" {
                pkgbuilds_with_pkgver_func.push(pkgbuild)
            }
        }
        Some(pkgbuilds_with_pkgver_func)
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
            self.filter_with_pkgver_func(actual_identity, dir).ok_or(())?;
        let _ = remove_dir_recursively("build");
        Self::extract_sources_many(actual_identity, &mut pkgbuilds)?;
        let children: Vec<Child> = pkgbuilds.iter().map(
        |pkgbuild|
            actual_identity.set_command(
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
        -> Result<(), ()> 
    {
        let mut pkgbuilds_need_build = vec![];
        let mut cleaners = vec![];
        let mut bad = false;
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
        if bad { Err(()) } else { Ok(()) }
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
        dir: P,
        holdgit: bool,
        skipint: bool,
        noclean: bool,
        proxy: Option<&str>,
        gmr: Option<&git::Gmr>
    ) -> Result<BaseRoot, ()> 
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
            &netfile_sources, &git_sources, holdgit, skipint, proxy, gmr)?;
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
        let base_root = BaseRoot::new(actual_identity)?;
        self.check_deps(actual_identity, dir.as_ref(), base_root.as_str())?;
        self.fill_all_ids_dirs();
        self.extract_if_need_build(actual_identity)?;
        if let Some(cleaners) = cleaners {
            for cleaner in cleaners {
                cleaner.join()
                .expect("Failed to join sources cleaner thread");
            }
        }
        Ok(base_root)
    }

    pub(super) fn build_any_needed<P: AsRef<Path>>(
        &mut self, actual_identity: &Identity, pkgbuilds_dir: P, base_root: &BaseRoot, nonet: bool
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
        let mut threads = vec![];
        for pkgbuild in self.0.iter() {
            if ! pkgbuild.extract {
                continue
            }
            let mut pkgbuild = pkgbuild.clone();
            if let Err(_) = wait_if_too_busy(
                &mut threads, 5, "bustr::FromStrilding packages") 
            {
                bad = true;
            }
            let actual_identity_thread = actual_identity.clone();
            threads.push(thread::spawn(move || 
                pkgbuild.build(&actual_identity_thread, nonet)));
        }
        if let Err(_) = threading::wait_remaining(
            threads, "building packages") 
        {
            bad = true;
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