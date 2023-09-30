use clap::Parser;

mod cksums;
mod download;
mod git;
mod identity;
mod pkgbuild;
mod source;
mod threading;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Arg {
    /// Optional PKGBUILDs.yaml file
    #[arg(default_value_t = String::from("PKGBUILDs.yaml"))]
    pkgbuilds: String,

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
}

fn main() {
    let arg = Arg::parse();
    if let Err(_) = pkgbuild::work(
        &arg.pkgbuilds, 
        arg.proxy.as_deref(),
        arg.holdpkg,
        arg.holdgit,
        arg.skipint,
        arg.nobuild,
        arg.noclean,
        arg.nonet,
        arg.gmr.as_deref()) 
    {
        std::process::exit(-1)
    }
}
