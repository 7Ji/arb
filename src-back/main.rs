use std::{env::ArgsOs, ffi::OsString, path::PathBuf, os::unix::ffi::OsStrExt};

mod applet_arb;
mod applet_builder;
mod applet_broker;
mod applet_init;
mod applet_pkgreader;

mod broker;
mod build;
mod child;
mod config;
mod pacman;
mod error;
mod filesystem;
mod init;
mod logfile;
mod environment;
mod pkgbuild;
mod root;
mod sign;
mod source;
mod threading;

use error::{
        Error,
        Result
    };


fn log_setup() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(
            "ARB_LOG_LEVEL", "info")
        ).target(env_logger::Target::Stdout).init();
}


fn clap_args(args: ArgsOs) -> impl Iterator<Item = OsString> {
    std::iter::once(OsString::new()).chain(args.into_iter())
}

fn dispatch(mut args: ArgsOs) -> Result<()> {
    let arg0 = match args.nth(0) {
        Some(arg0) => arg0,
        None => {
            log::error!("Failed to get arg0 to decide which applet to run");
            return Err(Error::InvalidArgument)
        },
    };
    let path = PathBuf::from(arg0);
    let name = match path.file_name() {
        Some(name) => name,
        None => {
            log::error!("Failed to get name from path '{}' to decide which \
                applet to run", path.display());
            return Err(Error::InvalidArgument)
        },
    };
    let name = name.as_bytes();
    match name {
        b"arb_multi" | b"arb-multi" | b"multi"  => dispatch(args),
        b"arb" | b"arch_repo_builder" | b"arch-repo-builder" => 
                    applet_arb::main(clap_args(args)),
        _ => {
            // For any other applet, we want to die with our parent
            if let Err(e) = nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGTERM) {
                log::error!("Failed to set parent detach signal to kill");
                return Err(e.into())
            }
            match name {
                b"broker" => applet_broker::main(),
                b"pkgreader" => applet_pkgreader::main(args),
                b"init" => applet_init::main(args),
                other => {
                    log::error!("Unknown applet {}", String::from_utf8_lossy(other));
                    Err(Error::InvalidArgument)
                },
            }
        }
    }
}

fn main() -> Result<()> {
    log_setup();
    dispatch(std::env::args_os())
}
