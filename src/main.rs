use clap::Parser;

mod cksums;
mod download;
mod git;
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

    /// Do not clean unused sources
    #[arg(short='C', long, default_value_t = false)]
    noclean: bool,
}

fn main() {
    let arg = Arg::parse();
    pkgbuild::work(
        &arg.pkgbuilds, 
        arg.proxy.as_deref(),
        arg.holdpkg,
        arg.holdgit,
        arg.skipint,
        arg.nobuild,
        arg.noclean);
}
