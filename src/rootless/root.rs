// Bootstrapping and erasing of root

use std::{iter::once, os::unix::fs::{symlink, DirBuilderExt}, path::{Path, PathBuf}};
use nix::{libc::mode_t, unistd::chroot};

use crate::{constant::PATH_PACMAN_CONF_UNDER_ROOT, filesystem::{file_create_with_content, set_permissions_mode}, pacman::PacmanConfig, rootless::RootlessHandler, Result};

use super::{idmap::IdMaps, BrokerPayload};


pub(crate) struct RootCommon {
    /// The `IdMaps` for this root, this is needed when we bootstrap/remove the
    /// root, so we have a full 65536 id space without actual root permission
    idmaps: IdMaps,
    /// The path where this root is stored, not necessarily where the target
    /// root would start from
    path: PathBuf,
    /// If this is not empty, then destroy self
    destory_with_exe: Option<PathBuf>,
}

pub(crate) type BaseRoot = RootCommon;

pub(crate) struct OverlayRoot {
    common: RootCommon,
    /// The merged, to-be-mounted-at root
    merged: PathBuf,
    /// The base root this is overlayed on
    base: PathBuf
}

pub(crate) trait Root {
    fn get_common(&self) -> &RootCommon;

    fn get_idmaps(&self) -> &IdMaps {
        &self.get_common().idmaps
    }

    /// Get the path where this root is stored, this might be diffrent from the
    /// result of `get_path_root`
    fn get_path_store(&self) -> &Path {
        &self.get_common().path
    }

    fn get_destroy_with_exe(&self) -> Option<&Path> {
        self.get_common().destory_with_exe.as_deref()
    }

    /// Get the path where the root would start from, this might be different
    /// from the result of `get_path_store`
    fn get_path_root(&self) -> &Path;

    fn get_path_pacman_conf(&self) -> PathBuf {
        self.get_path_root().join(PATH_PACMAN_CONF_UNDER_ROOT)
    }

    fn new_broker_payload(&self) -> BrokerPayload {
        BrokerPayload::new_with_root(&self.get_path_root())
    }

    fn set_permissions_mode<P: AsRef<Path>>(&self, suffix: P, mode: mode_t) 
        -> Result<()> 
    {
        set_permissions_mode(self.get_path_root().join(suffix), mode)
    }

    fn create_file_with_content<P, B>(&self, suffix: P, content: B) 
        -> Result<()> 
    where
        P: AsRef<Path>, 
        B: AsRef<[u8]>
    {
        file_create_with_content(self.get_path_root().join(suffix), content)
    }
}

impl Root for BaseRoot {
    fn get_common(&self) -> &RootCommon {
        self
    }

    fn get_path_root(&self) -> &Path {
        &self.path
    }
}

impl Root for OverlayRoot {
    fn get_common(&self) -> &RootCommon {
        &self.common
    }

    fn get_path_root(&self) -> &Path {
        &self.merged
    }
}

impl RootCommon {
    fn remove(&self) -> Result<()> {
        let exe = match self.get_destroy_with_exe() {
            Some(exe) => exe.to_owned(),
            None => return Ok(()),
        };
        let path = self.get_path_store();
        log::info!("Destroying root at '{}'", path.display());
        let rootless = RootlessHandler {
            idmaps: self.get_idmaps().clone(), exe,
        };
        let r = rootless.run_action_no_payload(
            "rm-rf", once(path)) ;
        if let Err(e) = &r {
            log::error!("Failed to destory root at '{}': {}",path.display(), e);
        }
        r
    }
}

impl Drop for RootCommon {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

impl BaseRoot {
    pub(crate) fn new<P1, P2>(
        path: P1, idmaps: &IdMaps, destroy_with_exe: Option<P2>
    ) -> Self 
    where
        P1: Into<PathBuf>,
        P2: Into<PathBuf>,
    {
        Self { 
            idmaps: idmaps.clone(), 
            path: path.into(), 
            destory_with_exe: destroy_with_exe.map(Into::into)
        }
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
        pacman_conf.try_write(self.get_path_pacman_conf())?;
        Ok(())
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