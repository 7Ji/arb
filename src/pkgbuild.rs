// TODO: Split this into multiple modules
// Progress: already splitted part into pkgbuild/parse.rs, add mod parse to enable part of that
use crate::{
        config::Pkgbuild as PkgbuildConfig,
        error::{
            Error,
            Result
        },
        identity::IdentityActual,
        source::{
            self,
            git::{self, Gmr},
            MapByDomain, Proxy,
        },
        root::{
            CommonRoot,
            BaseRoot,
            OverlayRoot, BootstrappingOverlayRoot,
        },
        threading::{
            self,
            wait_if_too_busy,
        }, filesystem::remove_dir_all_try_best, sign::sign_pkgs, depend::{Depends, DbHandle}, config::DepHashStrategy
    };
use git2::Oid;
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
// use super::{depend::Depends, DepHashStrategy};
// use super::depend::DbHandle;
// mod parse;


#[derive(Clone)]
enum Pkgver {
    Plain,
    Func { pkgver: String },
}

#[derive(Clone)]
pub(crate) struct PKGBUILD {
    pub(crate) base: String,
    branch: String,
    build: PathBuf,
    commit: git2::Oid,
    depends: Depends,
    pub(crate) extracted: bool,
    git: PathBuf,
    home_binds: Vec<String>,
    names: Vec<String>,
    pub(crate) need_build: bool,
    pub(crate) pkgid: String,
    pkgdir: PathBuf,
    pkgver: Pkgver,
    provides: Vec<String>,
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

impl AsRef<PKGBUILD> for PKGBUILD {
    fn as_ref(&self) -> &PKGBUILD {
        self
    }
}

impl PKGBUILD {
    // pub(crate) fn provides(&self, pkg: &String) -> bool {
    //     self.names.contains(pkg) || self.provides.contains(pkg)
    // }
    pub(crate) fn wants<'a> (&'a self, other: &'a Self) -> Option<&'a str> {
        for pkg in other.names.iter().chain(other.provides.iter()) {
            if self.depends.wants(pkg) {
                return Some(pkg)
            }
        }
        None
    }
    fn new(
        name: &str, url: &str, build_parent: &Path, git_parent: &Path,
        branch: Option<&str>, subtree: Option<&str>, deps: Option<&Vec<String>>,
        makedeps: Option<&Vec<String>>, home_binds: Option<&Vec<String>>,
        home_binds_global: &Vec<String>
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
        } else if url.starts_with("GH/") {
            if url.ends_with('/') {
                format!("https://github.com/{}{}.git", &url[3..], name)
            } else {
                format!("https://github.com/{}.git", &url[3..])
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
            home_binds: {
                let mut home_binds = match home_binds {
                    Some(home_binds) => home_binds.clone(),
                    None => vec![],
                };
                for home_bind in home_binds_global.iter() {
                    home_binds.push(home_bind.clone())
                }
                home_binds
            },
            names: vec![],
            need_build: false,
            pkgid: String::new(),
            pkgdir: PathBuf::from("pkgs"),
            pkgver: Pkgver::Plain,
            provides: vec![],
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
    fn healthy_get_commit(&self) -> Result<Oid> {
        let repo = match git::Repo::open_bare(
            &self.git, &self.url, None) 
        {
            Ok(repo) => repo,
            Err(e) => {
                log::error!("Failed to open or init bare repo {}",
                self.git.display());
                return Err(e.into())
            },
        };
        let commit = repo.get_branch_commit_or_subtree_id(
            &self.branch, self.subtree.as_deref()
        )?;
        match &self.subtree {
            Some(_) => log::info!("PKGBUILD '{}' at tree '{}'",
                        self.base, commit),
            None => log::info!("PKGBUILD '{}' at commit '{}'", self.base, commit),
        }
        if let Err(e) = repo.get_pkgbuild_blob(&self.branch,
                self.subtree.as_deref()) 
        {
            log::error!("Failed to get PKGBUILD blob");
            return Err(e)
        }
        Ok(commit)
    }

    fn healthy_set_commit(&mut self) -> Result<()> {
        match self.healthy_get_commit() {
            Ok(commit) => {
                self.commit = commit;
                Ok(())
            },
            Err(_e) => {
                // log::error!("PKGBUILD '{}' is not healthy: {}", &self.base, e);
                Err(Error::BrokenPKGBUILDs(vec![self.base.clone()]))
            },
        }
    }

    fn dump<P: AsRef<Path>> (&self, target: P) -> Result<()> {
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None)?;
        let blob = repo.get_pkgbuild_blob(&self.branch,
            self.subtree.as_deref())?;
        let mut file = match std::fs::File::create(&target) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to create file '{}' to dump PKGBUILD",
                    target.as_ref().display());
                return Err(e.into())
            },
        };
        if let Err(e) = file.write_all(blob.content()) {
            log::error!("Failed to write all content of blob '{}' into '{}': {}",
                blob.id(), target.as_ref().display(), e);
            return Err(e.into())
        }
        Ok(())
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
        -> Result<Vec<source::Source>>
    {
        source::get_sources(pkgbuild_file)
    }

    fn get_sources<P: AsRef<Path>> (&mut self, dir: P) -> Result<()> {
        let pkgbuild_file = dir.as_ref().join(&self.base);
        match Self::get_sources_file(&pkgbuild_file) {
            Ok(sources) => {
                self.sources = sources;
                Ok(())
            },
            Err(_) => Err(Error::BrokenPKGBUILDs(vec![self.base.clone()])),
        }
    }

    pub(crate) fn extractor_source(
        &self, actual_identity: &IdentityActual) -> Result<Child>
    {
        const SCRIPT: &str = include_str!("../scripts/extract_sources.bash");
        if let Err(e) = create_dir_all(&self.build) {
            log::error!("Failed to create build dir: {}", e);
            return Err(Error::IoError(e));
        }
        let repo = git::Repo::open_bare(
            &self.git, &self.url, None)?;
        repo.checkout(
            &self.build, &self.branch, self.subtree.as_deref()
        )?;
        source::extract(&self.build, &self.sources);
        let pkgbuild_dir = self.build.canonicalize().or_else(
        |e|{
            log::error!("Failed to canoicalize build dir path: {}", e);
            Err(Error::IoError(e))
        })?;
        let mut arg0 = OsString::from("[EXTRACTOR/");
        arg0.push(&self.base);
        arg0.push("] /bin/bash");
        let log_file = crate::logfile::LogFile::new(
            crate::logfile::LogType::Extract, &self.base)?;
        let dup_file = match log_file.file.try_clone() {
            Ok(dup_file) => dup_file,
            Err(e) => {
                log::error!("Failed to duplicate log file handle: {}", e);
                return Err(e.into())
            },
        };
        match actual_identity.set_root_drop_command(
            Command::new("/bin/bash")
                .arg0(&arg0)
                .arg("-ec")
                .arg(SCRIPT)
                .arg("Source extractor")
                .arg(&pkgbuild_dir))
                .stdout(dup_file)
                .stderr(log_file.file)
            .spawn()
        {
            Ok(child) => Ok(child),
            Err(e) => {
                log::error!("Faiiled to spawn extractor: {}", e);
                Err(Error::IoError(e))
            },
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
        log::info!("PKGBUILD '{}' pkgid is '{}'", self.base, self.pkgid);
    }

    pub(crate) fn get_temp_pkgdir(&self) -> Result<PathBuf> {
        let mut temp_name = self.pkgid.clone();
        temp_name.push_str(".temp");
        let temp_pkgdir = self.pkgdir.with_file_name(temp_name);
        let _ = remove_dir_all(&temp_pkgdir);
        match create_dir_all(&temp_pkgdir) {
            Ok(_) => Ok(temp_pkgdir),
            Err(e) => {
                log::error!("Failed to create temp pkgdir: {}", e);
                Err(e.into())
            },
        }
    }

    pub(crate) fn get_build_command(
        &self,
        actual_identity: &IdentityActual,
        temp_pkgdir: &Path
    )
        -> Result<Command>
    {
        let cwd = actual_identity.cwd();
        let cwd_no_root = actual_identity.cwd_no_root()?;
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
        actual_identity.set_root_chroot_drop_command(&mut command, chroot);
        Ok(command)
    }

    pub(crate) fn link_pkgs(&self) -> Result<()> {
        let mut rel = PathBuf::from("..");
        rel.push(&self.pkgid);
        let updated = PathBuf::from("pkgs/updated");
        // let mut bad = false;
        let readdir = match self.pkgdir.read_dir() {
            Ok(readdir) => readdir,
            Err(e) => {
                log::error!("Failed to read pkg dir: {}", e);
                return Err(e.into())
            },
        };
        let mut r = Ok(());
        for entry in readdir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    log::error!("Failed to read entry from pkg dir: {}", e);
                    r = Err(e.into());
                    continue
                },
            };
            let original = rel.join(entry.file_name());
            let link = updated.join(entry.file_name());
            if let Err(e) = symlink(&original, &link) {
                log::error!("Failed to symlink '{}' => '{}': {}",
                    link.display(), original.display(), e);
                r = Err(e.into());
            }
        }
        r
    }

    pub(crate) fn finish_build(&self,
        actual_identity: &IdentityActual, temp_pkgdir: &Path, sign: Option<&str>
    )
        -> Result<()>
    {
        log::info!("Finishing building '{}'", &self.pkgid);
        if self.pkgdir.exists() {
            if let Err(e) = remove_dir_all(&self.pkgdir) {
                log::error!("Failed to remove existing pkgdir: {}", e);
                return Err(e.into())
            }
        }
        if let Some(key) = sign {
            sign_pkgs(actual_identity, temp_pkgdir, key)?;
        }
        if let Err(e) = rename(&temp_pkgdir, &self.pkgdir) {
            log::error!("Failed to rename temp pkgdir '{}' to persistent pkgdir \
                '{}': {}", temp_pkgdir.display(), self.pkgdir.display(), e);
            return Err(e.into())
        }
        self.link_pkgs()?;
        log::info!("Finished building '{}'", &self.pkgid);
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

    pub(crate) fn _get_overlay_root(
        &self, actual_identity: &IdentityActual, nonet: bool
    ) -> Result<OverlayRoot>
    {
        OverlayRoot::_new(&self.base, actual_identity,
            &self.depends.needs, self.get_home_binds(), nonet)
    }

    pub(crate) fn get_bootstrapping_overlay_root(
        &self, actual_identity: &IdentityActual, nonet: bool
    ) -> Result<BootstrappingOverlayRoot>
    {
        BootstrappingOverlayRoot::new(&self.base, actual_identity,
            &self.depends.needs, self.get_home_binds(), nonet)
    }
}

// struct PkgsDepends (Vec<Depends>);
pub(crate) struct PKGBUILDs (pub(crate) Vec<PKGBUILD>);

impl PKGBUILDs {
    pub(crate) fn from_config(
        config: &HashMap<String, PkgbuildConfig>, home_binds_global: &Vec<String>
    )
        -> Result<Self>
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
                    None, home_binds_global
                ),
                PkgbuildConfig::Complex { url, branch,
                    subtree, deps,
                    makedeps,
                    home_binds,binds: _
                } => PKGBUILD::new(
                    name, url, &build_parent, &git_parent,
                    branch.as_deref(), subtree.as_deref(),
                    deps.as_ref(), makedeps.as_ref(), home_binds.as_ref(), home_binds_global)
            }
        }).collect();
        pkgbuilds.sort_unstable_by(
            |a, b| a.base.cmp(&b.base));
        Ok(Self(pkgbuilds))
    }

    fn sync(&self, hold: bool, proxy: Option<&Proxy>, gmr: Option<&Gmr>, terminal: bool)
        -> Result<()>
    {
        let map =
            PKGBUILD::map_by_domain(&self.0);
        let repos_map =
            match git::ToReposMap::to_repos_map(
                map, "sources/PKGBUILD", gmr)
        {
            Ok(repos_map) => repos_map,
            Err(e) => {
                log::error!("Failed to convert PKGBUILDs to repos map");
                return Err(e.into())
            },
        };
        git::Repo::sync_mt(repos_map, hold, proxy, terminal)
    }

    fn healthy_set_commit(&mut self) -> Result<()> {
        let mut broken = vec![];
        for pkgbuild in self.0.iter_mut() {
            if let Err(e) = pkgbuild.healthy_set_commit() {
                if let Error::BrokenPKGBUILDs(mut pkgbuilds) = e {
                    broken.append(&mut pkgbuilds)
                }
            }
        }
        if broken.is_empty() {
            Ok(())
        } else {
            Err(Error::BrokenPKGBUILDs(broken))
        }
    }

    pub(crate) fn from_config_healthy(
        config: &HashMap<String, PkgbuildConfig>,
        hold: bool, noclean: bool, proxy: Option<&Proxy>, gmr: Option<&Gmr>,
        home_binds: &Vec<String>, terminal: bool
    ) -> Result<Self>
    {
        let mut pkgbuilds = Self::from_config(config, home_binds)?;
        let update_pkg = if hold {
            if let Err(e) = pkgbuilds.healthy_set_commit() {
                log::error!("Warning: holdpkg set, but PKGBUILDs unhealthy, \
                           need update: {}", e);
                true
            } else {
                log::info!(
                    "Holdpkg set and all PKGBUILDs healthy, no need to update");
                false
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
            if let Err(e) = pkgbuilds.sync(hold, proxy, gmr, terminal) {
                log::error!("Failed to sync PKGBUILDs: {}", e);
                return Err(e)
            }
            if let Err(e) = pkgbuilds.healthy_set_commit() {
                log::error!("Updating broke some of our PKGBUILDs: {}", e);
                return Err(e)
            }
        }
        if let Some(cleaner) = cleaner {
            cleaner.join()
                .expect("Failed to join PKGBUILDs cleaner thread");
        }
        Ok(pkgbuilds)
    }

    fn dump<P: AsRef<Path>> (&self, dir: P) -> Result<()> {
        let dir = dir.as_ref();
        let mut r = Ok(());
        for pkgbuild in self.0.iter() {
            let target = dir.join(&pkgbuild.base);
            if let Err(e) = pkgbuild.dump(&target) {
                log::error!("Failed to dump PKGBUILD '{}' to '{}'",
                    pkgbuild.base, target.display());
                r = Err(e)
            }
        }
        r
    }

    fn get_deps<P: AsRef<Path>> (
        &mut self, actual_identity: &IdentityActual, dir: P, db_handle: &DbHandle,
        dephash_strategy: &DepHashStrategy
    ) -> Result<()>
    {
        let mut r = Ok(());
        let mut children = vec![];
        for pkgbuild in self.0.iter() {
            match pkgbuild.dep_reader(actual_identity, &dir) {
                Ok(child) => children.push(child),
                Err(e) => {
                    log::error!(
                        "Failed to spawn dep reader for PKGBUILD '{}': {}",
                        pkgbuild.base, e);
                    r = Err(e.into())
                },
            }
        }
        if r.is_err() {
            for mut child in children {
                if let Err(e) = child.kill() {
                    log::error!("Failed to kill child: {}", e);
                    r = Err(e.into())
                }
            }
            return r
        }
        if self.0.len() != children.len() {
            return Err(Error::ImpossibleLogic)
        }
        // let mut all_deps = vec![];
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
                        log::info!("PKGBUILD '{}' needed dependencies: {:?}",
                                &pkgbuild.base, &pkgbuild.depends.needs);
                    } else {
                        log::info!("PKGBUILD '{}' dephash {:016x}, \
                                needed dependencies: {:?}",
                                &pkgbuild.base, pkgbuild.depends.hash,
                                &pkgbuild.depends.needs);
                    }
                },
                Err(e) => {
                    log::error!("Failed to get needed deps for package '{}'",
                            &pkgbuild.base);
                    r = Err(e.into())
                },
            }
        }
        r

    }

    fn check_deps<P: AsRef<Path>> (
        &mut self, actual_identity: &IdentityActual, dir: P, root: P,
        dephash_strategy: &DepHashStrategy
    )   -> Result<()>
    {
        let db_handle = DbHandle::new(root)?;
        self.get_deps(actual_identity, dir, &db_handle, dephash_strategy)
    }

    fn get_all_sources<P: AsRef<Path>> (&mut self, dir: P)
      -> Result<(Vec<source::Source>, Vec<source::Source>, Vec<source::Source>)>
    {
        let mut sources_non_unique = vec![];
        for pkgbuild in self.0.iter_mut() {
            if let Err(e) = pkgbuild.get_sources(&dir) {
                log::error!("Failed to get sources for PKGBUILD '{}'",
                    pkgbuild.base);
                return Err(e);
            } else {
                for source in pkgbuild.sources.iter() {
                    sources_non_unique.push(source);
                }
            }
        }
        source::unique_sources(&sources_non_unique)
    }

    fn filter_with_pkgver_func<P: AsRef<Path>>(
        &mut self, actual_identity: &IdentityActual, dir: P
    ) -> Result<Vec<&mut PKGBUILD>>
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
                log::error!("Failed to spawn child to read pkgver types: {}", e);
                return Err(e.into())
            },
        };
        let mut child_in = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                log::error!("Failed to take child stdin");
                if let Err(e) = child.kill() {
                    log::error!("Failed to kill child: {}", e);
                }
                return Err(Error::ImpossibleLogic)
            },
        };
        let mut child_out = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                log::error!("Failed to take child stdout");
                if let Err(e) = child.kill() {
                    log::error!("Failed to kill child: {}", e);
                }
                return Err(Error::ImpossibleLogic)
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
                    log::error!("Failed to write buffer to child: {}", e);
                    if let Err(e) = child.kill() {
                        log::error!("Failed to kill child: {}", e);
                    }
                    return Err(e.into())
                },
            }
            match child_out.read(&mut output_buffer[..]) {
                Ok(read_this) =>
                    output.extend_from_slice(&output_buffer[0..read_this]),
                Err(e) => {
                    log::error!("Failed to read stdout child: {}", e);
                    if let Err(e) = child.kill() {
                        log::error!("Failed to kill child: {}", e);
                    }
                    return Err(e.into())
                },
            }
        }
        drop(child_in);
        match child_out.read_to_end(&mut output) {
            Ok(_) => (),
            Err(e) => {
                log::error!("Failed to read stdout child: {}", e);
                if let Err(e) = child.kill() {
                    log::error!("Failed to kill child: {}", e);
                }
                return Err(e.into())
            },
        }
        let status = match child.wait() {
            Ok(status) => status,
            Err(e) => {
                log::error!(
                    "Failed to wait for child reading pkgver type: {}", e);
                return Err(e.into())
            },
        };
        match status.code() {
            Some(code) => {
                if code != 0 {
                    log::error!("Reader bad return");
                    return Err(Error::BadChild { pid: None, code: Some(code) })
                }
            },
            None => {
                log::error!("Failed to get return code from child reading \
                        pkgver type");
                return Err(Error::ImpossibleLogic)
            },
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
        -> Result<()>
    {
        let mut children = vec![];
        let mut r = Ok(());
        for pkgbuild in pkgbuilds.iter_mut() {
            match pkgbuild.extractor_source(actual_identity) {
                Ok(child) => children.push(child),
                Err(e) => {
                    log::error!("Failed to spawn source extractor: {}", e);
                    r = Err(e)
                },
            }
        }
        for mut child in children {
            if let Err(e) = child.wait() {
                log::error!("Failed to wait for child: {}", e);
                r = Err(e.into())
            }
        }
        r
    }

    fn fill_all_pkgvers<P: AsRef<Path>>(
        &mut self, actual_identity: &IdentityActual, dir: P
    )
        -> Result<()>
    {
        let mut pkgbuilds =
            self.filter_with_pkgver_func(actual_identity, dir)?;
        Self::extract_sources_many(actual_identity, &mut pkgbuilds)?;
        let children: Vec<Child> = pkgbuilds.iter().map(
        |pkgbuild| {
            log::info!("Executing pkgver() for '{}'...", &pkgbuild.base);
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
            log::info!("PKGBUILD '{}' pkgver is '{}'", &pkgbuild.base, &pkgver);
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
        -> Result<u32>
    {
        let mut cleaners = vec![];
        let mut r = Ok(0);
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
                log::info!("Skipped already built '{}'",
                    pkgbuild.pkgdir.display());
                if pkgbuild.extracted {
                    pkgbuild.extracted = false;
                    let dir = pkgbuild.build.clone();
                    if let Err(e) = wait_if_too_busy(
                        &mut cleaners, 30,
                        "cleaning builddir") {
                        r = Err(e);
                    }
                    cleaners.push(thread::spawn(||
                        remove_dir_all_try_best(dir)));
                }
            } else {
                pkgbuild.need_build = true;
                need_build += 1;
            }
        }
        if let Err(e) = threading::wait_remaining(
            cleaners, "cleaning builddirs")
        {
            r = Err(e)
        }
        if r.is_ok() {
            r = Ok(need_build)
        }
        r
    }

    pub(crate) fn prepare_sources(
        &mut self,
        actual_identity: &IdentityActual,
        basepkgs: &Vec<String>,
        holdgit: bool,
        skipint: bool,
        noclean: bool,
        proxy: Option<&Proxy>,
        gmr: Option<&git::Gmr>,
        dephash_strategy: &DepHashStrategy,
        terminal: bool
    ) -> Result<Option<BaseRoot>>
    {

        let dir = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(e) => {
                log::error!("Failed to create temp dir to dump PKGBUILDs: {}", e);
                return Err(e.into())
            },
        };
        let cleaner = match
            PathBuf::from("build").exists()
        {
            true => Some(thread::spawn(|| remove_dir_all_try_best("build"))),
            false => None,
        };
        self.dump(&dir)?;
        let (netfile_sources, git_sources, _)
            = self.get_all_sources(&dir)?;
        source::cache_sources_mt(
            &netfile_sources, &git_sources, actual_identity,
            holdgit, skipint, proxy, gmr, terminal)?;
        if let Some(cleaner) = cleaner {
            match cleaner.join() {
                Ok(r) => if let Err(e) = r {
                    log::error!("Build dir cleaner failed: {}", e);
                    return Err(e)
                },
                Err(e) => {
                    log::error!("Failed to join build dir cleaner thread");
                    return Err(Error::ThreadFailure(Some(e)));
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

    pub(crate) fn clean_pkgdir(&self) {
        let mut used: Vec<String> = self.0.iter().map(
            |pkgbuild| pkgbuild.pkgid.clone()).collect();
        used.push(String::from("updated"));
        used.push(String::from("latest"));
        used.sort_unstable();
        source::remove_unused("pkgs", &used);
    }

    pub(crate) fn link_pkgs(&self) {
        let rel = PathBuf::from("..");
        let latest = PathBuf::from("pkgs/latest");
        for pkgbuild in self.0.iter() {
            if ! pkgbuild.pkgdir.exists() {
                continue;
            }
            let dirent = match pkgbuild.pkgdir.read_dir() {
                Ok(dirent) => dirent,
                Err(e) => {
                    log::error!("Failed to read dir '{}': {}",
                        pkgbuild.pkgdir.display(), e);
                    continue
                },
            };
            let rel = rel.join(&pkgbuild.pkgid);
            for entry in dirent {
                if let Ok(entry) = entry {
                    let original = rel.join(entry.file_name());
                    let link = latest.join(entry.file_name());
                    if let Err(e) = symlink(&original, &link) {
                        log::error!("Failed to link '{}' => '{}': {}",
                            link.display(), original.display(), e);
                    }
                }
            }
        }
    }
}