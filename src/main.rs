mod aur;
mod checksum;
mod child;
mod cli;
mod config;
mod error;
mod filesystem;
mod git;
mod io;
mod logfile;
mod mount;
mod pkgbuild;
mod pacman;
mod proxy;
mod rootless;
mod threading;
mod worker;

use error::{Error, Result};

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .filter_or("ARB_LOG_LEVEL", "info")
    ).target(env_logger::Target::Stderr).init();
    cli::work()
}
