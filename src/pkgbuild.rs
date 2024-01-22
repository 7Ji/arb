use std::collections::HashMap;
use git2::Oid;
use serde::Deserialize;
use pkgbuild;
use url::Url;
use crate::{Error, Result, git::{Repo, ReposMap}, proxy::Proxy};

pub(crate) mod reader;

/// The static part that comes from config
#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum PkgbuildConfig {
    /// A simple name:url PKGBUILD
    Simple (String),
    /// An advanced PKGBUILD
    Complex {
        url: String,
        #[serde(default)]
        branch: String,
        #[serde(default)]
        subtree: String,
        #[serde(default)]
        deps: Vec<String>,
        #[serde(default)]
        makedeps: Vec<String>,
        #[serde(default)]
        homebinds: Vec<String>,
    },
}

pub(crate) type PkgbuildsConfig = HashMap<String, PkgbuildConfig>;

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
    fn from_config(name: String, config: PkgbuildConfig) -> Self 
    {
        let (mut url, mut branch, mut subtree, 
            deps, makedeps, homebinds) = 
        match config {
            PkgbuildConfig::Simple(url) => (
                url, Default::default(), Default::default(), Default::default(), 
                Default::default(), Default::default()),
            PkgbuildConfig::Complex { url, branch, 
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

impl From<PkgbuildsConfig> for Pkgbuilds {
    fn from(config: PkgbuildsConfig) -> Self {
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
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Sync the PKGBUILDs repos from remote
    pub(crate) fn sync(&self, gmr: &str, holdpkg: bool, proxy: &Proxy) 
        -> Result<()> 
    {
        ReposMap::try_from(self)?.sync_mt(gmr, proxy, holdpkg)?;
        Ok(())
    }
}