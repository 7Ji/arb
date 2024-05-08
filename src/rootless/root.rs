// Bootstrapping and erasing of root

use std::{fs::{create_dir, set_permissions, Permissions}, iter::once, os::unix::fs::{symlink, PermissionsExt}, path::{Path, PathBuf}};
use crate::{pacman::PacmanConfig, rootless::RootlessHandler, Result};

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

    pub(crate) fn get_path(&self) -> &Path {
        &self.path
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
        set_permissions(self.path.join("dev/shm"), PermissionsExt::from_mode(0o1777))?;
        set_permissions(self.path.join("tmp"), PermissionsExt::from_mode(0o1777))?;
        set_permissions(self.path.join("proc"), PermissionsExt::from_mode(0o555))?;
        set_permissions(self.path.join("sys"), PermissionsExt::from_mode(0o555))?;
        symlink("../../../../pacman.sync", 
            self.path.join("var/lib/pacman/sync"))?;
        let path_pacman_conf = self.get_path_pacman_conf();
        let mut pacman_conf = pacman_conf.clone();
        pacman_conf.set_root(self.path.to_string_lossy());
        pacman_conf.to_file(&path_pacman_conf)?;
        Ok(())
    }

    /// As we operate in the ancestor naming space, we do not have any mounting
    /// related to the root, we can just simply remove everything
    fn remove(&self) -> Result<()> {
        let exe = match &self.destory_with_exe {
            Some(exe) => exe.clone(),
            None => return Ok(()),
        };
        log::info!("Destroying root at '{}'", self.path.display());
        let rootless = RootlessHandler {
            idmaps: self.idmaps.clone(), exe,
        };
        if let Err(e) = rootless.run_action(
            "rm-rf", once(&self.path)) 
        {
            log::error!("Failed to destory root at '{}': {}",
                self.path.display(), e);
            Err(e.into())
        } else {
            Ok(())
        }
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}