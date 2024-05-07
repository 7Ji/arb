// Bootstrapping and erasing of root

use std::{ffi::OsStr, fs::{create_dir, create_dir_all}, path::{Path, PathBuf}};
use crate::{filesystem::remove_dir_all_try_best, Error, Result};

use super::idmap::IdMaps;

/// An Arch Linux root, 
pub(crate) struct Root {
    /// The `IdMaps` for this root, this is needed when we bootstrap/remove the
    /// root, so we have a full 65536 id space without actual root permission
    idmaps: IdMaps,
    path: PathBuf,
}

/// Similar to `Root` but would be removed when going out of scope
pub(crate) struct TemporaryRoot {
    inner: Root,
}

impl Root {
    pub(crate) fn new<P: AsRef<Path>>(path: P, idmaps: &IdMaps) -> Self {
        Self { idmaps: idmaps.clone(), path: path.as_ref().into() }
    }

    /// Bootstrap this root, with an optional alternative `pacman.conf` to the
    /// default `/etc/pacman.conf`, and a list of packages to install.
    /// 
    /// Only when the list of packages is empty, a default `base` package would 
    /// be installed
    fn bootstrap<P, I, S>(&self, pacman_conf: Option<P>, pkgs: I) -> Result<()> 
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>
    {
        create_dir(&self.path)?;
        for suffix in &[
            "dev", "dev/pts", "dev/shm", 
            "etc", "etc/pacman.d",
            "proc",
            "run",
            "sys",
            "tmp",
            "var", "var/cache", "var/cache/pacman", "var/cache/pacman/pkg",
            "var/lib", "var/lib/pacman",
            "var/log"
        ] {
            create_dir(self.path.join(suffix))?
        }


        Ok(())
    }

    /// This is deprecated, as the host is not guaranteed to be Arch Linux
    fn clone_host() {

    }

    /// As we operate in the ancestor naming space, we do not have any mounting
    /// related to the root, we can just simply remove everything
    fn remove(&self) -> Result<()> {
        remove_dir_all_try_best(&self.path)
    }
}