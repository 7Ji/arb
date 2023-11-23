use std::{
        ffi::OsStr,
        fs::create_dir,
        path::PathBuf,
    };

use crate::error::{
        Error,
        Result
    };

pub(super) struct BuildDir {
    pub(super) path: PathBuf,
}

impl BuildDir {
    pub(super) fn new<S: AsRef<OsStr>>(name: S) -> Result<Self> {
        let path = PathBuf::from("build").join(name.as_ref());
        if path.exists() {
            if ! path.is_dir() {
                log::error!("Existing path for build dir is not a dir");
                return Err(Error::FilesystemConflict)
            }
        } else {
            if let Err(e) = create_dir(&path) {
                log::error!("Failed to create build dir: {}", e);
                return Err(e.into())
            }
        }
        Ok(Self { path })
    }

    pub(super) fn prepare() -> Result<()> {
        crate::filesystem::create_dir_allow_existing("build")
    }
}

impl Drop for BuildDir {
    fn drop(&mut self) {
        if crate::filesystem::remove_dir_all_try_best(&self.path).is_err() {
            log::error!("Warning: failed to remove build dir '{}'",
                self.path.display())
        }
    }
}