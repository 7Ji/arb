// Bootstrapping and erasing of root

use std::{iter::once, os::unix::fs::{symlink, DirBuilderExt}, path::{Path, PathBuf}};
use nix::{libc::mode_t, unistd::chroot};

use crate::{filesystem::{file_create_with_content, set_permissions_mode}, pacman::PacmanConfig, rootless::RootlessHandler, Result};

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

    fn set_permissions_mode<P: AsRef<Path>>(&self, suffix: P, mode: mode_t) 
        -> Result<()> 
    {
        set_permissions_mode(self.path.join(suffix), mode)
    }

    pub(crate) fn create_file_with_content<P, B>(&self, suffix: P, content: B) 
        -> Result<()> 
    where
        P: AsRef<Path>, 
        B: AsRef<[u8]>
    {
        file_create_with_content(self.path.join(suffix), content)
    }

    pub(crate) fn prepare_layout(&self, pacman_conf: &PacmanConfig) 
        -> Result<()> 
    {
        // FS layout
        let mut builder = std::fs::DirBuilder::new();
        builder.create(&self.path)?;
        for suffix in &[
            "dev",
            "etc", "etc/pacman.d",
            "run",
            "var", "var/cache", "var/cache/pacman", "var/cache/pacman/pkg",
            "var/lib", "var/lib/pacman", "var/lib/pacman/sync",
            "var/log"
        ] {
            builder.create(self.path.join(suffix))?
        }
        builder.mode(0o1777);
        builder.create(self.path.join("tmp"))?;
        builder.mode(0o555);
        builder.create(self.path.join("proc"))?;
        builder.create(self.path.join("sys"))?;
        // Configs
        symlink("/usr/share/zoneinfo/UTC", 
            self.path.join("etc/localtime"))?;
        self.create_file_with_content("etc/hostname", "arb")?;
        self.create_file_with_content(
            "etc/locale.conf", "LANG=en_GB.UTF-8")?;
        self.create_file_with_content(
            "etc/locale.gen", "en_GB.UTF-8 UTF-8")?;
        let mut pacman_conf = pacman_conf.clone();
        pacman_conf.set_root(self.path.to_string_lossy());
        pacman_conf.to_file(self.get_path_pacman_conf())?;
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
        if let Err(e) = rootless.run_action_no_payload(
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

pub(crate) fn chroot_checked<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if let Err(e) = chroot(path) {
        log::error!("Failed to chroot '{}': {}", path.display(), e);
        Err(e.into())
    } else {
        Ok(())
    }
}