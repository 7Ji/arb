mod arch_repo_builder;
mod aur;
mod error;
mod filesystem;
mod git;
mod io;
mod pkgbuild;
mod pacman;
mod proxy;
mod rootless;
mod threading;

use std::{env::ArgsOs, path::PathBuf, os::unix::ffi::OsStrExt, ffi::OsString};

use error::{Error, Result};

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// The all-in-one builder, an end-user should only run this
    Main (arch_repo_builder::Args),
    #[clap(subcommand, hide = true)]
    Broker,
    #[clap(subcommand, hide = true)]
    /// A psuedo init implementaion
    Init,
}

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .filter_or("ARB_LOG_LEVEL", "info")
    ).target(env_logger::Target::Stderr).init();
    // dispatch(std::env::args_os())
    let args: Args = clap::Parser::parse();
    Ok(())
}
