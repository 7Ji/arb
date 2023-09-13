use clap::Parser;

use tempfile::tempdir;

mod git;
mod pkgbuild;
mod source;
mod cksums;
mod download;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Arg {
    /// Optional PKGBUILDs.yaml file
    #[arg(default_value_t = String::from("PKGBUILDs.yaml"))]
    pkgbuilds: String,

    /// HTTP proxy to retry for git updating and http(s) netfiles if attempt without proxy failed
    #[arg(short, long)]
    proxy: Option<String>,

    /// Hold versions of PKGBUILDs, do not update them
    #[arg(short='P', long, default_value_t = false)]
    holdpkg: bool,

    /// Hold versions of git sources, do not update them
    #[arg(short='G', long, default_value_t = false)]
    holdgit: bool,

    /// Skip integrity check for netfile sources if they're found
    #[arg(short='s', long, default_value_t = false)]
    skipint: bool
}

fn main() {let arg = Arg::parse();
    let proxy = arg.proxy.as_deref();
    let pkgbuilds = pkgbuild::get_pkgbuilds(&arg.pkgbuilds, arg.holdpkg, proxy);
    let pkgbuilds_dir = tempdir().expect("Failed to create temp dir to dump PKGBUILDs");
    pkgbuild::prepare_sources(pkgbuilds_dir, &pkgbuilds, arg.holdgit, arg.skipint, proxy);
}
