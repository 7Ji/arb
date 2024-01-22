mod arch_repo_builder;
mod aur;
mod error;
mod filesystem;
mod git;
mod io;
mod pkgbuild;
mod proxy;
mod rootless;
mod threading;

use std::{env::ArgsOs, path::PathBuf, os::unix::ffi::OsStrExt, ffi::OsString};

use error::{Error, Result};

/// Add a pseudo `arg0` before the remaining args, otherwise clap would assume
/// the first arg is `arg0`
fn clap_args(args: ArgsOs) -> impl Iterator<Item = OsString> {
    std::iter::once(OsString::new()).chain(args)
}

/// Dispatch multi-call applet based on `arg0`, strip one arg and shift args
/// to the left if `arg0` appears to be a dispatcher itself.
fn dispatch(mut args: ArgsOs) -> Result<()> {
    let path: PathBuf = match args.nth(0) {
        Some(arg0) => arg0.into(),
        None => {
            log::error!("Failed to get arg0 to decide which applet to run");
            return Err(Error::InvalidArgument)
        },
    };
    let name = match path.file_name() {
        Some(name) => name,
        None => {
            log::error!("Failed to get name from path '{}' to decide which \
                applet to run", path.display());
            return Err(Error::InvalidArgument)
        },
    };
    let name_bytes = name.as_bytes();
    match name_bytes {
        // The ancestor 'applet', we should continue dispatching
        b"arb_multi" | b"arb-multi" | b"multi" => 
            return dispatch(args),
        // The builder applet, the main applet responsible for creating others
        b"arb" | b"arch_repo_builder" | b"arch-repo-builder" => 
            return arch_repo_builder::applet(clap_args(args)),
        _ => ()
    }
    // Other applet are all considered child, we want to die with our parent
    if let Err(e) = nix::sys::prctl::set_pdeathsig(
        nix::sys::signal::Signal::SIGTERM) 
    {
        log::error!("Failed to set parent detach signal to kill");
        return Err(e.into())
    }
    match name_bytes {
        // b"broker" => applet_broker::main(),
        b"pkgbuild_reader" => pkgbuild::reader::applet(),
        // b"init" => applet_init::main(args),
        _ => {
            log::error!("Unknown applet {}", name.to_string_lossy());
            Err(Error::InvalidArgument)
        },
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .filter_or("ARB_LOG_LEVEL", "info")
    ).target(env_logger::Target::Stderr).init();
    dispatch(std::env::args_os())
}
