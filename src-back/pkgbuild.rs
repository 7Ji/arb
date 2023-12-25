// Wrap around 7Ji/pkgbuild-rs

use std::collections::HashMap;
use crate::{
        config::{
            Config,
            Pkgbuild as PkgbuildConfig,
        },
        error::{
            Error,
            Result,
        },
    };


pub(crate) struct Pkgbuild {
    inner: pkgbuild::Pkgbuild,
    url: String,
    branch: String,
    commit: git2::Oid,
}

pub(crate) struct Pkgbuilds {
    inner: Vec<Pkgbuild>
}

impl TryFrom<&HashMap<String, PkgbuildConfig>> for Pkgbuilds {
    type Error = Error;

    fn try_from(value: &HashMap<String, PkgbuildConfig>) -> Result<Self> {
        todo!()
    }
}

impl TryFrom<&Config> for Pkgbuilds {
    type Error = Error;

    fn try_from(value: &Config) -> Result<Self> {
        value.pkgbuilds.try_into()
    }
}


impl Pkgbuilds {
    fn from_config(config: &Config) -> Result<Self> {
        config.try_into()
    }

    fn from_pkgbuilds_config(pkgbuilds: &HashMap<String, PkgbuildConfig>) 
        -> Result<Self>
    {
        pkgbuilds.try_into()
    }
}