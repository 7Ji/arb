use std::{ffi::OsString, fs::create_dir, io::{stdout, Read, Write}, iter::once, path::Path};
use git2::Oid;
use nix::unistd::setgid;
use pkgbuild;
use crate::{config::{PersistentPkgbuildConfig, PersistentPkgbuildsConfig}, filesystem::{chdir, create_dir_allow_existing}, git::{Repo, ReposMap}, mount::mount_bind, proxy::Proxy, rootless::{chroot_checked, set_uid_gid, try_unshare_user_mount_and_wait, BrokerPayload}, Error, Result};

#[derive(Debug)]
pub(crate) struct Pkgbuild {
    inner: pkgbuild::Pkgbuild,
    /// This is the name defined in config, not necessarily the same as 
    /// `inner.base`
    name: String, 
    url: String,
    branch: String,
    subtree: String,
    deps: Vec<String>,
    makedeps: Vec<String>,
    homebinds: Vec<String>,
    commit: Oid,
}

impl Pkgbuild {
    fn from_config(name: String, config: PersistentPkgbuildConfig) -> Self 
    {
        let (mut url, mut branch, mut subtree, 
            deps, makedeps, homebinds) = 
        match config {
            PersistentPkgbuildConfig::Simple(url) => (
                url, Default::default(), Default::default(), Default::default(), 
                Default::default(), Default::default()),
            PersistentPkgbuildConfig::Complex { url, branch, 
                subtree, deps, makedeps, 
                homebinds } => (
                    url, branch, subtree, deps, makedeps, homebinds),
        };
        if url  == "AUR" {
            url = format!("https://aur.archlinux.org/{}.git", name)
        } else if url.starts_with("GITHUB/") {
            if url.ends_with('/') {
                url = format!(
                    "https://github.com/{}{}.git", &url[7..], name)
            } else {
                url = format!(
                    "https://github.com/{}.git", &url[7..])
            }
        } else if url.starts_with("GH/") {
            if url.ends_with('/') {
                url = format!(
                    "https://github.com/{}{}.git", &url[3..], name)
            } else {
                url = format!(
                    "https://github.com/{}.git", &url[3..])
            }
        }
        if branch.is_empty() {
            branch = "master".into()
        }
        if subtree.ends_with('/') {
            subtree.push_str(&name)
        }
        subtree = subtree.trim_start_matches('/').into();
        Self {
            inner: Default::default(),
            name,
            url,
            branch,
            subtree,
            deps,
            makedeps,
            homebinds,
            commit: git2::Oid::zero(),
        }
    }

    fn dump<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let repo: Repo = self.try_into()?;
        repo.dump_branch_pkgbuild(
            &self.branch, self.subtree.as_ref(), path.as_ref())
    }
}

impl TryInto<Repo> for &Pkgbuild {
    type Error = Error;

    fn try_into(self) -> Result<Repo> {
        Repo::try_new_with_url_branch(&self.url, "PKGBUILD", &self.branch)
    }
}

#[derive(Default, Debug)]
pub(crate) struct Pkgbuilds {
    pub(crate) pkgbuilds: Vec<Pkgbuild>,
}

impl From<PersistentPkgbuildsConfig> for Pkgbuilds {
    fn from(config: PersistentPkgbuildsConfig) -> Self {
        let mut pkgbuilds: Vec<Pkgbuild> = config.into_iter().map(
            |(name, config)| {
                Pkgbuild::from_config(name, config)
            }).collect();
        pkgbuilds.sort_unstable_by(
            |a,b|a.name.cmp(&b.name));
        Self {
            pkgbuilds,
        }
    }
}

impl Into<Vec<Pkgbuild>> for Pkgbuilds {
    fn into(self) -> Vec<Pkgbuild> {
        self.pkgbuilds
    }
}

impl Pkgbuilds {
    /// Generate a 7Ji/git-mirroer config
    pub(crate) fn gengmr(&self) -> String {
        log::info!("Generateing 7Ji/git-mirrorer config...");
        let mut repos: Vec<String> = self.pkgbuilds.iter().map(
            |repo|repo.url.clone()).collect();
        repos.sort_unstable();
        let mut buffer = String::new();
        buffer.push_str("repos:\n");
        for repo in repos.iter() {
            buffer.push_str("  - ");
            buffer.push_str(repo);
            buffer.push('\n');
        }
        buffer
    }

    pub(crate) fn complete_from_reader<R: Read>(&mut self, reader: R) -> Result<()> {
        let pkgbuilds_raw: Vec<pkgbuild::Pkgbuild> = match rmp_serde::from_read(reader) {
            Ok(pkgbuilds_raw) => pkgbuilds_raw,
            Err(e) => {
                log::error!("Failed to read raw PKGBUILDs from reader: {}", e);
                return Err(e.into())
            },
        };
        let count_parsed = pkgbuilds_raw.len();
        let count_config = self.pkgbuilds.len();
        if count_parsed != count_config {
            log::error!("Parsed PKGBUILDs count different from input: \
                parsed {}, config {}", count_parsed, count_config);
            return Err(Error::ImpossibleLogic)
        }
        for (pkgbuild_wrapper, pkgbuild_raw) in 
            self.pkgbuilds.iter_mut().zip(pkgbuilds_raw.into_iter()) 
        {
            pkgbuild_wrapper.inner = pkgbuild_raw;
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pkgbuilds.is_empty()
    }

    /// Sync the PKGBUILDs repos from remote
    pub(crate) fn sync(&self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        ReposMap::from_iter(self.pkgbuilds.iter())?.sync(gmr, proxy, hold)?;
        Ok(())
    }

    pub(crate) fn dump<P: AsRef<Path>>(&self, parent: P) -> Result<()> {
        let parent = parent.as_ref();
        create_dir(&parent)?;
        for pkgbuild in self.pkgbuilds.iter() {
            let path_pkgbuild = parent.join(&pkgbuild.name);
            pkgbuild.dump(&path_pkgbuild)?
        }
        Ok(())
    }

    pub(crate) fn get_reader_payload(&self, root: &Path) -> BrokerPayload {
        let mut payload = BrokerPayload::new_with_root(root);
        let root: OsString = root.into();
        payload.add_init_command_run_applet(
            "read-pkgbuilds",
            once(root).chain(
            self.pkgbuilds.iter().map(
                |pkgbuild|(&pkgbuild.name).into())));
        payload
    }
}

/// The `pkgbuild_reader` applet entry point, takes no args
pub(crate) fn action_read_pkgbuilds<P1, I, P2>(root: P1, pkgbuilds: I) 
    -> Result<()> 
where
    P1: AsRef<Path>,
    I: IntoIterator<Item = P2>,
    P2: AsRef<Path>
{
    // Out parent (init) cannot chroot, as that would result in parent init 
    // possibly failing to call us, due to libc, libgit, etc being in different 
    // versions
    log::info!("Reading PKGBUILDs...");
    let path_root = root.as_ref();
    let path_pkgbuilds = path_root.join("PKGBUILDs");
    create_dir_allow_existing(&path_pkgbuilds)?;
    mount_bind("build/PKGBUILDs", &path_pkgbuilds)?;
    chdir(&path_pkgbuilds)?;
    chroot_checked("..")?;
    set_uid_gid(1000, 1000)?;
    let pkgbuilds = match pkgbuild::parse_multi(pkgbuilds)
    {
        Ok(pkgbuilds) => pkgbuilds,
        Err(e) => {
            log::error!("Failed to parse PKGBUILDs: {}", e);
            return Err(e.into())
        },
    };
    let output = match rmp_serde::to_vec(&pkgbuilds) {
        Ok(output) => output,
        Err(e) => {
            log::error!("Failed to encode output: {}", e);
            return Err(e.into())
        },
    };
    if let Err(e) = stdout().write_all(&output) {
        log::error!("Failed to write serialized output to stdout: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}