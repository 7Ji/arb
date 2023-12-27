use std::{collections::HashMap, fs::File, path::Path};

use serde::Deserialize;

use crate::error::{
    Error,
    Result
};

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

fn default_basepkgs() -> Vec<String> {
    vec![String::from("base-devel")]
}

impl TryFrom<&Path> for Config {
    type Error = Error;

    fn try_from(path: &Path) -> Result<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to open config file '{}': {}", 
                        path.display(), e);
                return Err(e.into())
            },
        };
        match serde_yaml::from_reader(file) {
            Ok(config) => Ok(config),
            Err(e) => {
                log::error!("Failed to parse YAML config file '{}': {}", 
                    path.display(), e);
                Err(e.into())
            },
        }
    }
}

impl Config {
    pub(crate) fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        path.as_ref().try_into()
    }
}