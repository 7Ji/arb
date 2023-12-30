use std::io::{stdin, stdout, Write};
use crate::{Error, Result};

pub(crate) type Input = Vec<String>;
pub(crate) type Output = Vec<pkgbuild::Pkgbuild>;

/// The `pkgbuild_reader` applet entry point, takes no args
pub(crate) fn applet() -> Result<()> 
{
    let input: Input = match rmp_serde::from_read(stdin()) {
        Ok(input) => input,
        Err(e) => {
            log::error!("Failed to decode input from stdin: {}", e);
            return Err(e.into())
        },
    };
    let pkgbuilds = match pkgbuild::parse_multi(&input) {
        Ok(pkgbuilds) => pkgbuilds,
        Err(e) => {
            log::error!("Failed to parse PKGBUILDs: {}", e);
            return Err(e.into())
        },
    };
    if input.len() != pkgbuilds.len() {
        // pkgbuild-rs guarantees the count does not change
        // I don't want panic here, just return error if that really changed
        log::error!("Read PKGBUILDs cound mismatch, impossible");
        return Err(Error::ImpossibleLogic)
    }
    let output = match rmp_serde::to_vec(&pkgbuilds) {
        Ok(output) => output,
        Err(e) => {
            log::error!("Failed to encode output: {}", e);
            return Err(e.into())
        },
    };
    if let Err(e) = stdout().write_all(&output) {
        log::error!("Failed to write serialized output to stdout: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}