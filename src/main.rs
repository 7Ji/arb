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

use std::{env::ArgsOs, ffi::OsString, os::unix::ffi::OsStrExt, path::{Path, PathBuf}};

use error::{Error, Result};

// fn action_read_config<P: AsRef<Path>>(path_config: P) -> {

// }

#[derive(Default)]
enum ActionState {
    #[default]
    None,
    ReadConfig,
    FetchedPkgbuilds,
    FetchedSources,

}


impl ActionState {
    fn read_config<P: AsRef<Path>>(self, path_config: P) -> Result<Self> {
        if let Self::None = self {
            Ok(Self::ReadConfig)
        } else {
            panic!()
        }
    }

    fn fetch_pkgbuilds(self) -> Result<Self> {
        if let Self::ReadConfig = self {
            Ok(Self::FetchedPkgbuilds)
        } else {
            panic!()
        }
    }

    fn fetch_sources(self) -> Result<Self> {
        if let Self::FetchedPkgbuilds = self {
            Ok(Self::FetchedSources)
        } else {
            panic!()
        }
    }
}

#[derive(clap::Args, Debug, Clone)]
struct ActionArgs {
    /// The path to config file
    config: String,
    /// Only do action for the specific PKGBUILD(s), for all if none is set
    pkgbuilds: Vec<String>,
}

impl ActionArgs {
    fn read_config(&self) -> Result<()> {
        ActionState::default()
            .read_config(&self.config)?;
        Ok(())
    }

    fn fetch_pkgbuilds(&self) -> Result<()> {
        ActionState::default()
            .read_config(&self.config)?
            .fetch_pkgbuilds();
        Ok(())
    }

    fn fetch_sources(&self) -> Result<()> {
        ActionState::default()
            .read_config(&self.config)?
            .fetch_pkgbuilds()?
            .fetch_sources()?;
        Ok(())
    }

    // fn fetch_pkgs(&self) -> Result<()> {
    //     ActionState::default()
    //         .read_config(&self.config)?
    //         .fetch_pkgbuilds()?
    //         .fetch_sources();
    //     Ok(())
    // }

    fn make_base_chroot(&self) -> Result<()> {
        Ok(())
    }

    fn make_chroots(&self) -> Result<()> {
        Ok(())
    }

    fn build(&self) -> Result<()> {
        Ok(())
    }

    fn release(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Action {
    /// Fetch PKBUILDs
    FetchPkgbuilds (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then fetch sources
    FetchSources (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then fetch dependent pkgs
    FetchPkgs (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then make base chroot
    MakeBaseChroot (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then make PKGBUILD-specific chroots
    MakeChroots (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then build PKGBUILDs
    Build (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then create release
    Release (
        #[command(flatten)]
        ActionArgs
    ),
    /// Do everything above. End users should only use this instead of the above split actions
    DoEverything (
        #[command(flatten)]
        ActionArgs
    ),
    /// A simple init implementation
    #[clap(hide = true)]
    Init {
        args: Vec<String>,
    },
}

#[derive(clap::Parser, Debug)]
#[command(version)]
struct Arg {
    #[command(subcommand)]
    action: Action,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .filter_or("ARB_LOG_LEVEL", "info")
    ).target(env_logger::Target::Stderr).init();
    // dispatch(std::env::args_os())
    let arg: Arg = clap::Parser::parse();
    match &arg.action {
        Action::FetchPkgbuilds(args) => args.fetch_pkgbuilds(),
        Action::FetchSources(args) => args.fetch_sources(),
        Action::FetchPkgs(args) => args.fetch_pkgs(),
        Action::MakeBaseChroot(args) => args.make_base_chroot(),
        Action::MakeChroots(args) => args.make_chroots(),
        Action::Build(args) => args.build(),
        Action::Release(args) => args.release(),
        Action::DoEverything(args) => args.release(),
        Action::Init { args } => todo!(),
    }?;
    // match &arg.action {
    //     Action::FetchPkgbuilds => todo!(),
    //     Action::FetchSources => todo!(),
    //     Action::FetchPkgs => todo!(),
    //     Action::MakeBaseChroot => todo!(),
    //     Action::MakeChroots => todo!(),
    //     Action::Build => todo!(),
    //     Action::Release => todo!(),
    //     Action::DoEverything => todo!(),
    //     Action::_Init => todo!(),
    // }
    Ok(())
}
