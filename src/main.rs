use std::process::exit;

use clap::Parser;
use serde::Deserialize;

mod build;
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

    /// HTTP proxy to retry for git updating and http(s)
    /// netfiles if attempt without proxy failed
    #[arg(short, long)]
    proxy: Option<String>,

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
    pkgbuilds: std::collections::HashMap<String, build::PkgbuildConfig>,
}

fn main() {
    let actual_identity = match 
        identity::Identity::get_actual_and_drop() 
    {
        Ok(identity) => identity,
        Err(_) => {
            eprintln!("Failed to get and drop to non-root actual identity");
            Arg::parse_from(["--help"]);
            exit(-1);
        },
    };
    let arg = Arg::parse();
    let file = match std::fs::File::open(&arg.config) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to open config file '{}': {}", arg.config, e);
            exit(-1);
        },
    };
    let config: Config = match serde_yaml::from_reader(file) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to parse YAML: {}", e);
            exit(-1)
        },
    };
    if let Err(_) = build::work(
        actual_identity,
        &config.pkgbuilds,
        arg.proxy.as_deref().or(config.proxy.as_deref()),
        arg.holdpkg || config.holdpkg,
        arg.holdgit || config.holdgit,
        arg.skipint || config.skipint,
        arg.nobuild || config.nobuild,
        arg.noclean || config.noclean,
        arg.nonet || config.nonet,
        arg.gmr.as_deref().or(config.gmr.as_deref()),
        arg.sign.as_deref().or(config.sign.as_deref())) 
    {
        std::process::exit(-1)
    }
}
