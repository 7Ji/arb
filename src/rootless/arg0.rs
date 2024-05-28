use std::{fs::read_link, path::PathBuf};
use crate::Result;

const PATH_EXE: &str = "/proc/self/exe";

pub(crate) fn try_get_arg0() -> Result<PathBuf> {
    match read_link(PATH_EXE) {
        Ok(exe) => Ok(exe),
        Err(e) => {
            log::error!("Failed to read link '{}' to get \
                actual arg0: {}", PATH_EXE, e);
            Err(e.into())
        },
    }
}

pub(crate) fn get_arg0() -> PathBuf {
    try_get_arg0().unwrap_or(PATH_EXE.into())
}