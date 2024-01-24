use std::{env::ArgsOs, ffi::{OsString, OsStr}, collections::HashMap, fs::{read_link, File}};

use clap::Parser;
use serde::Deserialize;

use crate::{Error, Result, pkgbuild::{PkgbuildConfig, PkgbuildsConfig, Pkgbuild, Pkgbuilds}, filesystem, proxy::Proxy};

// Note: a lot of options is Option<T> instead of T, e.g. Option<String>, this
// is to differentiate them from undefined values so we could later combine
// config and args to get a joint settings.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Only build the specified package(s), can be specified multiple times,
    /// implies --noclean
    #[arg(short, long)]
    build: Option<Vec<String>>,

    /// Path for the config file
    #[arg(short, long, default_value_t = String::from("config.yaml"))]
    config: String,

    /// Generate a list of Git repos that could be used by 7Ji/git-mirrorer on
    /// stdout
    #[arg(long)]
    gengmr: bool,

    /// Prefix of a 7Ji/git-mirrorer instance, e.g. git://gmr.lan,
    /// the mirror would be tried first before actual git remotes
    #[arg(short='g', long)]
    gmr: Option<String>,

    /// Hold versions of git sources, do not update them
    #[arg(short='G', long)]
    holdgit: Option<bool>,

    /// Hold versions of PKGBUILDs, do not update them
    #[arg(short='P', long)]
    holdpkg: Option<bool>,

    /// Skip integrity check for netfile sources if they're found
    #[arg(short='I', long)]
    lazyint: Option<bool>,

    /// Attempt without proxy for this amount of tries before actually using
    /// the proxy
    #[arg(short='X', long)]
    lazyproxy: Option<usize>,

    /// Do not actually build the packages after extraction
    #[arg(short='B', long)]
    nobuild: Option<bool>,

    /// Do not clean unused sources and outdated packages
    #[arg(short='C', long)]
    noclean: Option<bool>,

    /// Disallow any network connection during build routine
    #[arg(short='N', long)]
    nonet: Option<bool>,

    /// Path to pacman.conf
    #[arg(long)]
    paconf: Option<String>,

    /// Proxy for git updating and http(s), currently support only http
    #[arg(short, long)]
    proxy: Option<String>,

    /// The GnuPG key ID used to sign packages
    #[arg(short, long)]
    sign: Option<String>,
}

/// Config structure read from `config.yaml`, most importantly containing
/// definition of `PKGBUILD`s
/// 
/// This should not be directly used by any logic other than reading config
#[derive(Debug, PartialEq, Deserialize)]
pub(crate) struct PersistentConfig {
    basepkgs: Option<Vec<String>>,
    gmr: Option<String>,
    holdgit: Option<bool>,
    holdpkg: Option<bool>,
    homebinds: Option<Vec<String>>,
    lazyint: Option<bool>,
    lazyproxy: Option<usize>,
    nobuild: Option<bool>,
    noclean: Option<bool>,
    nonet: Option<bool>,
    paconf: Option<String>,
    pkgbuilds: PkgbuildsConfig,
    proxy: Option<String>,
    sign: Option<String>,
}

impl TryFrom<&OsStr> for PersistentConfig {
    type Error = Error;

    fn try_from(value: &OsStr) -> Result<Self> {
        let file = match File::open(value) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to open config file '{}': {}", 
                    value.to_string_lossy(), e);
                return Err(e.into())
            },
        };
        match serde_yaml::from_reader(file) {
            Ok(config) => Ok(config),
            Err(e) => {
                log::error!("Failed to parse config file '{}': {}",
                    value.to_string_lossy(), e);
                Err(e.into())
            },
        }
    }
}

/// Unified CLI temporary + file persistent config
pub(crate) struct Config {
    pub(crate) basepkgs: Vec<String>,
    pub(crate) build: Vec<String>,
    pub(crate) gengmr: bool,
    pub(crate) gmr: String,
    pub(crate) holdgit: bool,
    pub(crate) holdpkg: bool,
    pub(crate) homebinds: Vec<String>,
    pub(crate) lazyint: bool,
    pub(crate) nobuild: bool,
    pub(crate) noclean: bool,
    pub(crate) nonet: bool,
    pub(crate) paconf: String,
    pub(crate) pkgbuilds: Pkgbuilds,
    pub(crate) proxy: Proxy,
    pub(crate) sign: String,
}

impl Config {
    /// Consumes an `Args` and a `PersistentConfig` and release the memory
    /// 
    /// It's possible to write an alternative method to not consume either of
    /// the inputs, but that would keep the memory occupied, which is not what I
    /// like.
    fn from_args_and_persistent(args: Args, persistent: PersistentConfig) 
        -> Result<Self>
    {
        let config = Self {
            basepkgs: persistent.basepkgs.unwrap_or_default(),
            build: args.build.unwrap_or_default(),
            gengmr: args.gengmr,
            gmr: args.gmr.unwrap_or(persistent.gmr.unwrap_or_default()),
            holdgit: args.holdgit.unwrap_or(
                persistent.holdgit.unwrap_or_default()),
            holdpkg: args.holdpkg.unwrap_or(
                persistent.holdpkg.unwrap_or_default()),
            homebinds: persistent.homebinds.unwrap_or_default(),
            lazyint: args.lazyint.unwrap_or(
                persistent.lazyint.unwrap_or_default()),
            nobuild: args.nobuild.unwrap_or(
                persistent.nobuild.unwrap_or_default()),
            noclean: args.noclean.unwrap_or(
                persistent.noclean.unwrap_or_default()),
            nonet: args.nonet.unwrap_or(persistent.nonet.unwrap_or_default()),
            paconf: args.paconf.unwrap_or(
                persistent.paconf.unwrap_or(
                    "/etc/pacman.conf".into())),
            pkgbuilds: persistent.pkgbuilds.into(),
            proxy: Proxy {
                url:args.proxy.unwrap_or(persistent.proxy.unwrap_or_default()), 
                after: args.lazyproxy.unwrap_or(
                    persistent.lazyproxy.unwrap_or_default())
            },
            sign: args.sign.unwrap_or(persistent.sign.unwrap_or_default()),
        };
        Ok(config)
    }

    fn from_args<I: Iterator<Item = OsString>>(args: I) -> Result<Self> {
        let args = Args::parse_from(args);
        let persistent = 
            PersistentConfig::try_from(args.config.as_ref())?;
        Self::from_args_and_persistent(args, persistent)
    }
}

/// The `arb`/`arch_repo_builder` applet entry point
pub(crate) fn applet<I>(args: I) -> Result<()> 
where 
    I: Iterator<Item = OsString>
{
    // Read arg, read persistent config, and combine them into runtime config
    let mut config = Config::from_args(args)?;
    if config.pkgbuilds.is_empty() { 
        log::error!("No PKGBUILDs defined");
        return Err(Error::InvalidConfig)
    }
    if config.gengmr {
        config.pkgbuilds.gengmr()
    }
    let rootless_handler = crate::rootless::Handler::new()?;
    // Basic layout
    filesystem::prepare_layout()?;
    let mut pacman_config = crate::pacman::Config::from_file(&config.paconf)?;
    pacman_config.set_cache_dir_here();
    pacman_config.to_file("build/pacman.cache.conf")?;
    println!("{}", pacman_config);
    // Sync PKGBUILDs
    config.pkgbuilds.sync(&config.gmr, &config.proxy, config.holdpkg)?;
    config.pkgbuilds.complete()?;
    Ok(())
}