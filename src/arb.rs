// Applet: arb, arch_repo_builder

use std::collections::HashMap;

use clap::Parser;

use serde::Deserialize;

use crate::error::{
        Error,
        Result,
    };

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Arg {
    /// Optional config.yaml file
    #[arg(default_value_t = String::from("config.yaml"))]
    config: String,

    /// Optional packages to only build them, implies --noclean
    #[arg(short, long)]
    build: Vec<String>,

    /// HTTP proxy to retry for git updating and http(s)
    /// netfiles if attempt without proxy failed
    #[arg(short, long)]
    proxy: Option<String>,

    /// Attempt without proxy for this amount of tries before actually using
    /// the proxy, to save bandwidth
    #[arg(long)]
    proxy_after: Option<usize>,

    /// Hold versions of PKGBUILDs, do not update them
    #[arg(short='P', long, default_value_t = false)]
    holdpkg: bool,

    /// Hold versions of git sources, do not update them
    #[arg(short='G', long, default_value_t = false)]
    holdgit: bool,

    /// Skip integrity check for netfile sources if they're found
    #[arg(short='I', long, default_value_t = false)]
    skipint: bool,

    /// Do not actually build the packages
    #[arg(short='B', long, default_value_t = false)]
    nobuild: bool,

    /// Do not clean unused sources and outdated packages
    #[arg(short='C', long, default_value_t = false)]
    noclean: bool,

    /// Disallow any network connection during makepkg's build routine
    #[arg(short='N', long, default_value_t = false)]
    nonet: bool,

    /// Drop to the specific uid:gid pair, instead of getting from SUDO_UID/GID
    #[arg(short='d', long)]
    drop: Option<String>,

    /// Prefix of a 7Ji/git-mirrorer instance, e.g. git://gmr.lan,
    /// The mirror would be tried first before actual git remote
    #[arg(short='g', long)]
    gmr: Option<String>,

    /// The GnuPG key ID used to sign packages
    #[arg(short, long)]
    sign: Option<String>
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConfigPkgbuild {
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
    pub(crate) dephash_strategy: crate::depend::DepHashStrategy,
    pub(crate) pkgbuilds: std::collections::HashMap<String, ConfigPkgbuild>,
    #[serde(default = "default_home_binds")]
    pub(crate) home_binds: Vec<String>,
}

fn default_basepkgs() -> Vec<String> {
    vec![String::from("base-devel")]
}

fn default_home_binds() -> Vec<String> {
    Vec::new()
}

struct Settings {
    actual_identity: crate::identity::IdentityActual,
    pkgbuilds_config: HashMap<String, ConfigPkgbuild>,
    basepkgs: Vec<String>,
    proxy: Option<crate::source::Proxy>,
    holdpkg: bool,
    holdgit: bool,
    skipint: bool,
    nobuild: bool,
    noclean: bool,
    nonet: bool,
    gmr: Option<String>,
    dephash_strategy: crate::depend::DepHashStrategy,
    sign: Option<String>,
    home_binds: Vec<String>,
    terminal: bool
}

fn prepare() -> Result<Settings> {
    let arg: Arg = clap::Parser::parse();
    let actual_identity =
    crate::identity::IdentityActual::new_and_drop(arg.drop.as_deref())?;
    let mut config: Config = serde_yaml::from_reader(
        std::fs::File::open(&arg.config).map_err(
        |e|{
            log::error!("Failed to open config file '{}': {}", arg.config, e);
            Error::IoError(e)
        })?)
    .or_else(
    |e|{
        log::error!("Failed to parse YAML: {}", e);
        Err(Error::InvalidConfig)
    })?;
    if ! arg.build.is_empty() {
        log::warn!("Only build the following packages: {:?}", arg.build);
        config.pkgbuilds.retain(|name, _|arg.build.contains(name));
    }
    let proxy = crate::source::Proxy::from_str_usize(
        arg.proxy.as_deref().or(config.proxy.as_deref()),
        match arg.proxy_after {
            Some(proxy_after) => proxy_after,
            None => match config.proxy_after {
                Some(proxy_after) => proxy_after,
                None => 0,
            },
        });
    Ok(Settings {
        actual_identity,
        pkgbuilds_config: config.pkgbuilds,
        basepkgs: config.basepkgs,
        proxy,
        holdpkg: arg.holdpkg || config.holdpkg,
        holdgit: arg.holdgit || config.holdgit,
        skipint: arg.skipint || config.skipint,
        nobuild: arg.nobuild || config.nobuild,
        noclean: !arg.build.is_empty() || arg.noclean || config.noclean,
        nonet: arg.nonet || config.nonet,
        gmr: arg.gmr.or(config.gmr),
        dephash_strategy: config.dephash_strategy,
        sign: arg.sign.or(config.sign),
        home_binds: config.home_binds,
        terminal: is_terminal::is_terminal(std::io::stdout())
    })
}

fn work(settings: Settings) -> Result<()> {
    let gmr = settings.gmr.and_then(|gmr|
        Some(crate::source::git::Gmr::init(gmr.as_str())));
    crate::filesystem::create_layout()?;
    let mut pkgbuilds =
        crate::pkgbuild::PKGBUILDs::from_config_healthy(
            &settings.pkgbuilds_config, settings.holdpkg,
            settings.noclean, settings.proxy.as_ref(),
            gmr.as_ref(), &settings.home_binds, settings.terminal)?;
    let root = pkgbuilds.prepare_sources(
        &settings.actual_identity, &settings.basepkgs, settings.holdgit,
        settings.skipint, settings.noclean, settings.proxy.as_ref(),
        gmr.as_ref(), &settings.dephash_strategy, settings.terminal)?;
    let r = crate::build::maybe_build(&pkgbuilds,
        root, &settings.actual_identity, settings.nobuild, settings.nonet,
         settings.sign.as_deref());
    let _ = std::fs::remove_dir("build");
    pkgbuilds.link_pkgs();
    if ! settings.noclean {
        pkgbuilds.clean_pkgdir();
    }
    r
}

pub(crate) fn main() -> Result<()> {
    work(prepare()?)
}