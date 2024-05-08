// Bootstrapping and erasing of root

use std::{ffi::OsStr, fs::{create_dir, create_dir_all}, path::{Path, PathBuf}, process::Command};
use crate::{filesystem::remove_dir_all_try_best, pacman::{install_pkgs, PacmanConfig}, rootless::RootlessHandler, Error, Result};

use super::idmap::IdMaps;

/// An Arch Linux root, 
pub(crate) struct Root {
    /// The `IdMaps` for this root, this is needed when we bootstrap/remove the
    /// root, so we have a full 65536 id space without actual root permission
    idmaps: IdMaps,
    path: PathBuf,
    /// If this is not empty, then destroy self
    destory_with_exe: Option<PathBuf>,
}

impl Root {
    pub(crate) fn new<P1, P2>(
        path: P1, idmaps: &IdMaps, destroy_with_exe: Option<P2>
    ) -> Self 
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        Self { 
            idmaps: idmaps.clone(), 
            path: path.as_ref().into(), 
            destory_with_exe: destroy_with_exe.and_then(
                |exe|Some(exe.as_ref().into())) 
        }
    }

    pub(crate) fn get_path_pacman_conf(&self) -> PathBuf {
        self.path.join("etc/pacman.conf")
    }

    pub(crate) fn prepare_layout(&self, pacman_conf: &PacmanConfig) 
        -> Result<()> 
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
        let path_pacman_conf = self.get_path_pacman_conf();
        let mut pacman_conf = pacman_conf.clone();
        pacman_conf.set_root(self.path.to_string_lossy());
        pacman_conf.to_file(&path_pacman_conf)?;
        Ok(())
    }

    /// As we operate in the ancestor naming space, we do not have any mounting
    /// related to the root, we can just simply remove everything
    fn remove(&self) -> Result<()> {
        remove_dir_all_try_best(&self.path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let exe = match &self.destory_with_exe {
            Some(exe) => exe.clone(),
            None => return,
        };
        log::info!("Destroying root at '{}'", self.path.display());
        let rootless = RootlessHandler {
            idmaps: self.idmaps.clone(), exe,
        };
        if let Err(e) = rootless.run_action(
            "rm-rf", &[&self.path], false) 
        {
            log::error!("Failed to destory root at '{}': {}",
                self.path.display(), e)
        }
    }
}