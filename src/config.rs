use std::{collections::HashMap, fs::File, path::Path};

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DepHash {
    Strict, // dep + makedep
    Loose,  // dep
    None,   // none
}

impl Default for DepHash {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum Pkgbuild {
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

#[derive(Debug, PartialEq, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) holdpkg: bool,
    #[serde(default)]
    pub(crate) holdgit: bool,
    #[serde(default)]
    pub(crate) skipint: bool,
    #[serde(default)]
    pub(crate) nobuild: bool,
    #[serde(default)]
    pub(crate) noclean: bool,
    #[serde(default)]
    pub(crate) nonet: bool,
    #[serde(default)]
    pub(crate) sign: String,
    #[serde(default)]
    pub(crate) gmr: String,
    #[serde(default)]
    pub(crate) proxy: String,
    #[serde(default)]
    pub(crate) lazyproxy: usize,
    #[serde(default = "default_basepkgs")]
    pub(crate) basepkgs: Vec<String>,
    #[serde(default)]
    pub(crate) dephash: DepHash,
    pub(crate) pkgbuilds: std::collections::HashMap<String, Pkgbuild>,
    #[serde(default = "default_home_binds")]
    pub(crate) homebinds: Vec<String>,
}

fn default_basepkgs() -> Vec<String> {
    vec![String::from("base-devel")]
}

fn default_home_binds() -> Vec<String> {
    Vec::new()
}

impl Config {
    pub(crate) fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to open config file '{}': {}", 
                        path.as_ref().display(), e);
                return Err(e.into())
            },
        };
        match serde_yaml::from_reader(file) {
            Ok(config) => Ok(config),
            Err(e) => {
                log::error!("Failed to parse YAML config file '{}': {}", 
                    path.as_ref().display(), e);
                Err(e.into())
            },
        }
    }
}