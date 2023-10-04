use std::{fs::File, collections::HashMap, path::Path};
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum Pkgbuild {
    Simple (String),
    Complex {
        url: String,
        branch: Option<String>,
        subtree: Option<String>,
        deps: Option<Vec<String>>,
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
    pub(crate) pkgbuilds: HashMap<String, Pkgbuild>,
}

impl Config {
    pub(crate) fn new<P: AsRef<Path>>(config_file: P) -> Result<Self, ()> {
        let file = File::open(config_file).or_else(|e|{
            eprintln!("Failed to open config file: {}", e);
            Err(())
        })?;
        serde_yaml::from_reader(file).or_else(|e|{
            eprintln!("Failed to parse YAML: {}", e);
            Err(())
        })
    }
}