use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DepHashStrategy {
    Strict, // dep + makedep
    Loose,  // dep
    None,   // none
}

impl Default for DepHashStrategy {
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
    pub(crate) sign: Option<String>,
    pub(crate) gmr: Option<String>,
    pub(crate) proxy: Option<String>,
    pub(crate) proxy_after: Option<usize>,
    #[serde(default = "default_basepkgs")]
    pub(crate) basepkgs: Vec<String>,
    #[serde(default)]
    pub(crate) dephash_strategy: DepHashStrategy,
    pub(crate) pkgbuilds: std::collections::HashMap<String, Pkgbuild>,
    pub(crate) home_binds: Vec<String>,
}

fn default_basepkgs() -> Vec<String> {
    vec![String::from("base-devel")]
}