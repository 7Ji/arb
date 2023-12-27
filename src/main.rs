mod arb;
mod error;
mod pkgbuild;

use std::{env::ArgsOs, path::PathBuf, os::unix::ffi::OsStrExt, ffi::OsString};

use error::{Error, Result};

/// Add a pseudo `arg0` before the remaining args, otherwise clap would assume
/// the first arg is `arg0`
fn clap_args(args: ArgsOs) -> impl Iterator<Item = OsString> {
    std::iter::once(OsString::new()).chain(args)
}

/// Dispatch multi-call applet based on `arg0`, strip one arg and shift args
/// to the left if `arg0` appears to be a multi-call applet.
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
    match name.as_bytes() {
        // The ancestor 'applet', we should continue dispatching
        b"arb_multi" | b"arb-multi" | b"multi" => 
            return dispatch(args),
        // The builder applet, the main applet responsible for creating others
        b"arb" | b"arch_repo_builder" | b"arch-repo-builder" => 
            return arb::applet(clap_args(args)),
        // Let's break out for shorter ident
        _ => ()
    }
    // Other applet are all considered child, we want to die with our parent
    if let Err(e) = nix::sys::prctl::set_pdeathsig(
        nix::sys::signal::Signal::SIGTERM) 
    {
        log::error!("Failed to set parent detach signal to kill");
        return Err(e.into())
    }
    // match name {
    //     b"broker" => applet_broker::main(),
    //     b"pkgreader" => applet_pkgreader::main(args),
    //     b"init" => applet_init::main(args),
    //     other => {
    //         log::error!("Unknown applet {}", String::from_utf8_lossy(other));
    //         Err(Error::InvalidArgument)
    //     },
    // }
    Ok(())
}

fn main() -> Result<()> {
    let mut logger = env_logger::Builder::from_env(
        env_logger::Env::default()
            .filter_or("ARB_LOG_LEVEL", "info"));
    #[cfg(feature = "log_stderr")]
    logger.target(env_logger::Target::Stderr).init();
    #[cfg(not(feature = "log_stderr"))]
    logger.target(env_logger::Target::Stdout).init();
    dispatch(std::env::args_os())
}
