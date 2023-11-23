mod arb;
mod build;
mod child;
mod depend;
mod error;
mod filesystem;
mod logfile;
mod identity;
mod pkgbuild;
mod root;
mod sign;
mod source;
mod threading;

use error::{
        Error,
        Result
    };

fn main_init() -> Result<()> {
    Ok(())
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(
            "ARB_LOG_LEVEL", "info")
        ).target(env_logger::Target::Stdout).init();
    match std::env::args().nth(0) {
        Some(arg0) => {
            let applet = match arg0.rsplit_once('/') {
                Some((_prefix, applet)) => applet,
                None => arg0.as_str(),
            };
            match applet {
                "arch_repo_builder" | "arb" => arb::main(),
                "init" => main_init(),
                _ => {
                    log::error!("Invalid applet {}", arg0.as_str());
                    Err(Error::BrokenEnvironment)
                },
            }
        },
        None => {
            log::error!("Could not get arg0 from environment");
            Err(Error::BrokenEnvironment)
        },
    }
}
