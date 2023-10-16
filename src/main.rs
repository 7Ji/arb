use clap::Parser;
use serde::Deserialize;

mod build;
mod child;
mod filesystem;
mod identity;
mod roots;
mod source;
mod threading;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Arg {
    /// Optional config.yaml file
    #[arg(default_value_t = String::from("config.yaml"))]
    config: String,

    /// Optional packages to only build them
    pkgs: Vec<String>,

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
struct Config {
    #[serde(default)]
    holdpkg: bool,
    #[serde(default)]
    holdgit: bool,
    #[serde(default)]
    skipint: bool,
    #[serde(default)]
    nobuild: bool,
    #[serde(default)]
    noclean: bool,
    #[serde(default)]
    nonet: bool,
    sign: Option<String>,
    gmr: Option<String>,
    proxy: Option<String>,
    proxy_after: Option<usize>,
    #[serde(default = "default_basepkgs")]
    basepkgs: Vec<String>,
    #[serde(default)]
    dephash_strategy: build::DepHashStrategy,
    pkgbuilds: std::collections::HashMap<String, build::PkgbuildConfig>,
}

fn default_basepkgs() -> Vec<String> {
    vec![String::from("base-devel")]
}

fn main() -> Result<(), &'static str> {
    let arg = Arg::parse();
    let actual_identity = 
    identity::IdentityActual::new_and_drop(arg.drop.as_deref())
    .or_else(|_|{
        Err("Failed to get actual identity")
    })?;
    let file = std::fs::File::open(&arg.config).or_else(
    |e|{
        eprintln!("Failed to open config file '{}': {}", arg.config, e);
        Err("Failed to open config file")
    })?;
    let mut config: Config = serde_yaml::from_reader(file).or_else(
    |e|{
        eprintln!("Failed to parse YAML: {}", e);
        Err("Failed to parse YAML config")
    })?;
    if ! arg.pkgs.is_empty() {
        println!("Only build the following packages: {:?}", arg.pkgs);
        config.pkgbuilds.retain(|name, _|arg.pkgs.contains(name));
    }
    let proxy = source::Proxy::from_str_usize(
        arg.proxy.as_deref().or(config.proxy.as_deref()), 
        match arg.proxy_after {
            Some(proxy_after) => proxy_after,
            None => match config.proxy_after {
                Some(proxy_after) => proxy_after,
                None => 0,
            },
        });
    build::work(
        actual_identity,
        &config.pkgbuilds,
        &config.basepkgs,
        proxy.as_ref(),
        arg.holdpkg || config.holdpkg,
        arg.holdgit || config.holdgit,
        arg.skipint || config.skipint,
        arg.nobuild || config.nobuild,
        arg.noclean || config.noclean,
        arg.nonet || config.nonet,
        arg.gmr.as_deref().or(config.gmr.as_deref()),
        &config.dephash_strategy,
        arg.sign.as_deref().or(config.sign.as_deref())
    ).or_else(|_|Err("Failed to build packages"))?;
    Ok(())
}
