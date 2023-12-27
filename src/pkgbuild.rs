use std::collections::HashMap;
use serde::Deserialize;
use pkgbuild;
use crate::{Error, Result};

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

/// The dynamic part that's generated from config
pub(crate) struct PkgbuildDynamic {
    inner: pkgbuild::Pkgbuild,
    commit: gix::ObjectId
}

pub(crate) struct Pkgbuild {
    inner: pkgbuild::Pkgbuild,
    url: String,
    branch: String,
    subtree: String,
    deps: Vec<String>,
    makedeps: Vec<String>,
    homebinds: Vec<String>,
    commit: gix::ObjectId,
}

impl Pkgbuild {
    fn from_config_and_dynamic(
        name: String, config: PkgbuildConfig, dynamic: PkgbuildDynamic
    ) -> Self 
    {
        const INIT_COMMIT: gix::ObjectId = gix::ObjectId::Sha1([0; 20]);
        let mut pkgbuild = match config {
            PkgbuildConfig::Simple(url) => Self {
                inner: Default::default(),
                url,
                branch: Default::default(),
                subtree: Default::default(),
                deps: Default::default(),
                makedeps: Default::default(),
                homebinds: Default::default(),
                commit: INIT_COMMIT,
            },
            PkgbuildConfig::Complex { 
                url, branch, subtree, deps, 
                makedeps, homebinds 
            } => Self {
                inner: Default::default(),
                url,
                branch,
                subtree,
                deps,
                makedeps,
                homebinds,
                commit: INIT_COMMIT,
            }
        };
        if pkgbuild.url  == "AUR" {
            pkgbuild.url = format!("https://aur.archlinux.org/{}.git", name)
        } else if pkgbuild.url.starts_with("GITHUB/") {
            if pkgbuild.url.ends_with('/') {
                pkgbuild.url = format!(
                    "https://github.com/{}{}.git", &pkgbuild.url[7..], name)
            } else {
                pkgbuild.url = format!(
                    "https://github.com/{}.git", &pkgbuild.url[7..])
            }
        } else if pkgbuild.url.starts_with("GH/") {
            if pkgbuild.url.ends_with('/') {
                pkgbuild.url = format!(
                    "https://github.com/{}{}.git", &pkgbuild.url[3..], name)
            } else {
                pkgbuild.url = format!(
                    "https://github.com/{}.git", &pkgbuild.url[3..])
            }
        }
        if pkgbuild.branch.is_empty() {
            pkgbuild.branch = "master".into()
        }
        if pkgbuild.subtree.ends_with('/') {
            pkgbuild.subtree.push_str(&name)
        }
        pkgbuild.subtree = pkgbuild.subtree.trim_start_matches('/').into();
        pkgbuild.inner.pkgbase = name;
        pkgbuild
    }
}

pub(crate) struct Pkgbuilds {
    entries: Vec<Pkgbuild>,
}

impl TryFrom<PkgbuildsConfig> for Pkgbuilds {
    type Error = Error;

    fn try_from(value: PkgbuildsConfig) -> Result<Self> {
        
        todo!()
    }
}