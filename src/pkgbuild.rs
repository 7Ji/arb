use std::collections::HashMap;
use git2::Oid;
use serde::Deserialize;
use pkgbuild;
use url::Url;
use crate::{config::{PersistentPkgbuildConfig, PersistentPkgbuildsConfig}, git::{Repo, ReposMap}, proxy::Proxy, Error, Result};

pub(crate) mod reader;

#[derive(Debug)]
pub(crate) struct Pkgbuild {
    pub(crate) inner: pkgbuild::Pkgbuild,
    /// This is the name defined in config, not necessarily the same as 
    /// `inner.base`
    pub(crate) name: String, 
    pub(crate) url: String,
    pub(crate) branch: String,
    pub(crate) subtree: String,
    pub(crate) deps: Vec<String>,
    pub(crate) makedeps: Vec<String>,
    pub(crate) homebinds: Vec<String>,
    pub(crate) commit: Oid,
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
        ReposMap::try_from(self)?.sync(gmr, proxy, hold)?;
        Ok(())
    }
}