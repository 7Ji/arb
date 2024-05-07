use std::{collections::HashMap, fs::create_dir, io::{stdout, Write}, path::{Path, PathBuf}};
use git2::Oid;
use serde::Deserialize;
use pkgbuild;
use url::Url;
use crate::{config::{PersistentPkgbuildConfig, PersistentPkgbuildsConfig}, filesystem::remove_dir_all_try_best, git::{Repo, ReposHashMap, ReposMap}, proxy::Proxy, Error, Result};

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
    pub(crate) entries: Vec<Pkgbuild>,
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
            entries: pkgbuilds,
        }
    }
}

impl Into<Vec<Pkgbuild>> for Pkgbuilds {
    fn into(self) -> Vec<Pkgbuild> {
        self.entries
    }
}

impl Pkgbuilds {
    /// Generate a 7Ji/git-mirroer config
    pub(crate) fn gengmr(&self) -> String {
        log::info!("Generateing 7Ji/git-mirrorer config...");
        let mut repos: Vec<String> = self.entries.iter().map(
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

    /// Complete the inner `PKGBUILD` for each PKGBUILD
    pub(crate) fn complete(&mut self) -> Result<()> {
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Sync the PKGBUILDs repos from remote
    pub(crate) fn sync(&self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        ReposMap::from_iter(self.entries.iter())?.sync(gmr, proxy, hold)?;
        Ok(())
    }

    pub(crate) fn dump<P: AsRef<Path>>(&self, parent: P) -> Result<()> {
        let parent = parent.as_ref();
        create_dir(&parent)?;
        for pkgbuild in self.entries.iter() {
            let path_pkgbuild = parent.join(&pkgbuild.name);
            pkgbuild.dump(&path_pkgbuild)?
        }
        remove_dir_all_try_best(&parent)?;
        Ok(())
    }
}

/// The `pkgbuild_reader` applet entry point, takes no args
pub(crate) fn action_read_pkgbuilds<I, P>(pkgbuilds: I) -> Result<()> 
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>
{
    crate::rootless::unshare_all_and_try_wait()?;
    let pkgbuilds = match pkgbuild::parse_multi(pkgbuilds) {
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