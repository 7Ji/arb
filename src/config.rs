use std::{collections::HashMap, fs::File, path::Path};

use pkgbuild::Architecture;
use serde::Deserialize;

use crate::{cli::ActionArgs, constant::{PATH_MAKEPKG_CONF, PATH_PACMAN_CONF}, filesystem::read_to_bytes, pacman::PacmanConfig, pkgbuild::Pkgbuilds, proxy::Proxy, Error, Result};

/// The static part that comes from config
#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum PersistentPkgbuildConfig {
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

pub(crate) type PersistentPkgbuildsConfig = 
    HashMap<String, PersistentPkgbuildConfig>;
/// Config structure read from `config.yaml`, most importantly containing
/// definition of `PKGBUILD`s
/// 
/// This should not be directly used by any logic other than reading config
#[derive(Debug, PartialEq, serde::Deserialize)]
pub(crate) struct PersistentConfig {
    arch: Option<String>,
    basepkgs: Option<Vec<String>>,
    gmr: Option<String>,
    holdgit: Option<bool>,
    holdpkg: Option<bool>,
    homebinds: Option<Vec<String>>,
    lazyint: Option<bool>,
    lazyproxy: Option<usize>,
    mpconf: Option<String>,
    nobuild: Option<bool>,
    noclean: Option<bool>,
    nonet: Option<bool>,
    paconf: Option<String>,
    pkgbuilds: PersistentPkgbuildsConfig,
    proxy: Option<String>,
    sign: Option<String>,
}

impl PersistentConfig {
    pub(crate) fn try_read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
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
                log::error!("Failed to parse config file '{}': {}", 
                    path.display(), e);
                Err(e.into())
            },
        }
    }
}
/// Unified CLI temporary + file persistent config
pub(crate) struct RuntimeConfig {
    pub(crate) arch: Architecture,
    pub(crate) basepkgs: Vec<String>,
    pub(crate) chosen: Vec<String>,
    pub(crate) gengmr: String,
    pub(crate) gmr: String,
    pub(crate) holdgit: bool,
    pub(crate) holdpkg: bool,
    pub(crate) homebinds: Vec<String>,
    pub(crate) lazyint: bool,
    pub(crate) nobuild: bool,
    pub(crate) noclean: bool,
    pub(crate) nonet: bool,
    pub(crate) mpconf: Vec<u8>,
    pub(crate) paconf: PacmanConfig,
    pub(crate) pkgbuilds: Pkgbuilds,
    pub(crate) proxy: Proxy,
    pub(crate) sign: String,
}

fn str_from_two_options<'a>(
    perferred: Option<&'a str>, less: Option<&'a str>, default: &'a str
) -> &'a str
{
    let str = perferred.unwrap_or(
        less.unwrap_or(
            ""
        )
    );
    if str.is_empty() {
        default
    } else {
        str
    }
}


impl TryFrom<(ActionArgs, PersistentConfig)> for RuntimeConfig {
    type Error = Error;

    fn try_from(value: (ActionArgs, PersistentConfig)) -> Result<Self> {
        let (args, persistent) = value;
        let mpconf = read_to_bytes(
            str_from_two_options(
                args.mpconf.as_deref(),
                persistent.mpconf.as_deref(),
                PATH_MAKEPKG_CONF))?;
        let paconf = PacmanConfig::try_read(
            str_from_two_options(
                args.paconf.as_deref(),
                persistent.paconf.as_deref(),
                PATH_PACMAN_CONF))?;
        let config = Self {
            arch: str_from_two_options(
                args.arch.as_deref(),
                persistent.arch.as_deref(),
                "auto").into(),
            basepkgs: persistent.basepkgs.unwrap_or_default(),
            chosen: args.chosen,
            gengmr: args.gengmr,
            gmr: args.gmr.unwrap_or(persistent.gmr.unwrap_or_default()),
            holdgit: args.holdgit.unwrap_or(
                persistent.holdgit.unwrap_or_default()),
            holdpkg: args.holdpkg.unwrap_or(
                persistent.holdpkg.unwrap_or_default()),
            homebinds: persistent.homebinds.unwrap_or_default(),
            lazyint: args.lazyint.unwrap_or(
                persistent.lazyint.unwrap_or_default()),
            mpconf,
            nobuild: args.nobuild.unwrap_or(
                persistent.nobuild.unwrap_or_default()),
            noclean: args.noclean.unwrap_or(
                persistent.noclean.unwrap_or_default()),
            nonet: args.nonet.unwrap_or(persistent.nonet.unwrap_or_default()),
            paconf,
            pkgbuilds: persistent.pkgbuilds.into(),
            proxy: Proxy::from_url_and_after(
                args.proxy.unwrap_or(persistent.proxy.unwrap_or_default()), 
                args.lazyproxy.unwrap_or(
                    persistent.lazyproxy.unwrap_or_default())
            ),
            sign: args.sign.unwrap_or(persistent.sign.unwrap_or_default()),
        };
        Ok(config)
    }
}

impl TryFrom<ActionArgs> for RuntimeConfig {
    type Error = Error;

    fn try_from(args: ActionArgs) -> Result<Self> {
        let persistent_config = 
            PersistentConfig::try_read(&args.config)?;
        Self::try_from((args, persistent_config))
    }
}