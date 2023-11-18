use std::{path::{Path, PathBuf}, process::Command, fs::{create_dir_all, create_dir}, ffi::OsStr};

use crate::identity::{IdentityActual, Identity};

use super::{mount::MountedFolder, common::CommonRoot};

use nix::mount::{mount, MsFlags};


/// The basic root, with bare-minimum packages installed
#[derive(Clone)]
pub(crate) struct BaseRoot (MountedFolder);


impl BaseRoot {
    fn path(&self) -> &Path {
        &self.0.0
    }

    /// Root is expected
    fn bind_self(&self) -> Result<&Self, ()> {
        mount(Some("roots/base"),
                self.path(),
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>)
        .map_err(|e|log::error!("Failed to mount base root: {}", e))?;
        Ok(self)
    }

    /// Root is expected
    fn remove(&self) -> Result<&Self, ()> {
        match self.0.remove() {
            Ok(_) => Ok(self),
            Err(_) => Err(()),
        }
    }

    /// Root is expected
    fn umount_recursive(&self) -> Result<&Self, ()> {
        match self.0.umount_recursive() {
            Ok(_) => Ok(self),
            Err(_) => Err(()),
        }
    }

    /// Root is expected
    fn create_home(&self, actual_identity: &IdentityActual)
        -> Result<&Self, ()>
    {
        // std::thread::sleep(std::time::Duration::from_secs(100));
        IdentityActual::run_chroot_command(
            Command::new("/usr/bin/mkhomedir_helper")
                .arg(actual_identity.name()),
            self.path())?;
        Ok(self)
    }

    /// Root is expected
    fn setup(&self, actual_identity: &IdentityActual) -> Result<&Self, ()> {
        log::warn!("Finishing base root setup");
        let builder = self.builder(actual_identity)?;
        self.copy_file_same("etc/passwd")?
            .copy_file_same("etc/group")?
            .copy_file_same("etc/shadow")?
            .copy_file_same("etc/makepkg.conf")?
            .create_home(actual_identity)?;
        create_dir_all(&builder)
            .or_else(|e|{
                log::error!("Failed to create chroot builder dir: {}", e);
                Err(())
            })?;
        for dir in Self::BUILDER_DIRS {
            create_dir(builder.join(dir))
                .or_else(|e|{
                    log::error!("Failed to create chroot builder dir: {}", e);
                    Err(())
                })?;
        }
        log::warn!("Finished base root setup");
        Ok(self)
    }

    pub(crate) fn db_only() -> Result<Self, ()> {
        IdentityActual::as_root(||MountedFolder::remove_all())?;
        log::info!("Creating base chroot (DB only)");
        let root = Self(MountedFolder(PathBuf::from("roots/base")));
        IdentityActual::as_root(||{
            root.remove()?
                .base_layout()?
                .bind_self()?
                .base_mounts()?
                .refresh_dbs()?;
            Ok(())
        })?;
        log::info!("Created base chroot (DB only)");
        Ok(root)
    }

    /// Create a base rootfs containing the minimum packages and user setup
    /// This should not be used directly for building packages
    pub(crate) fn _new<I, S>(actual_identity: &IdentityActual, pkgs: I)
        -> Result<Self, ()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>
    {
        IdentityActual::as_root(||MountedFolder::remove_all())?;
        log::info!("Creating base chroot");
        let root = Self(MountedFolder(PathBuf::from("roots/base")));
        IdentityActual::as_root(||{
            root.remove()?
                .base_layout()?
                .bind_self()?
                .base_mounts()?
                .refresh_dbs()?
                .install_pkgs(pkgs)?
                .setup(actual_identity)?
                .umount_recursive()?;
            Ok(())
        })?;
        log::info!("Created base chroot");
        Ok(root)
    }

    /// Finish a DB-only base root
    pub(crate) fn finish<I, S>(&self, actual_identity: &IdentityActual, pkgs: I)
        -> Result<&Self, ()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>
    {
        log::info!("Finishing base chroot");
        IdentityActual::as_root(||{
            self.install_pkgs(pkgs)?
                .setup(actual_identity)?
                .umount_recursive()?;
            Ok(())
        })?;
        log::info!("Finish base chroot");
        Ok(self)
    }
}

impl CommonRoot for BaseRoot {
    fn path(&self) -> &Path {
        self.0.0.as_path()
    }
}

impl Drop for BaseRoot {
    fn drop(&mut self) {
        let _ = IdentityActual::as_root(||MountedFolder::remove_all());
    }
}
