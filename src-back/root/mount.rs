use std::{
        fs::remove_dir_all,
        path::PathBuf, fmt::Display
    };
    
use nix::{
        mount::{
            mount,
            MsFlags,
        },
        NixPath
    };

use crate::{
        error::{
            Error,
            Result
        },
        identity::{
            Identity,
            IdentityActual,
        }
    };

#[derive(Clone)]
pub(super) struct MountedFolder (pub(super) PathBuf);

impl MountedFolder {
    /// Umount any folder starting from the path.
    /// Root is expected
    pub(super) fn umount_recursive(&self) -> Result<&Self> {
        log::info!("Umounting '{}' recursively...", self.0.display());
        let absolute_path = match self.0.canonicalize() {
            Ok(path) => path,
            Err(e) => {
                log::error!("Failed to canoicalize path '{}': {}",
                    self.0.display(), e);
                return Err(Error::IoError(e))
            },
        };
        let process = match procfs::process::Process::myself() {
            Ok(process) => process,
            Err(e) => {
                log::error!("Failed to get myself: {}", e);
                return Err(Error::ProcError(e))
            },
        };
        let mut exist = true;
        while exist {
            let mountinfos = match process.mountinfo() {
                Ok(mountinfos) => mountinfos,
                Err(e) => {
                    log::error!("Failed to get mountinfos: {}", e);
                    return Err(Error::ProcError(e))
                },
            };
            exist = false;
            for mountinfo in mountinfos.iter().rev() {
                if mountinfo.mount_point.starts_with(&absolute_path) {
                    if let Err(e) = nix::mount::umount(
                        &mountinfo.mount_point)
                    {
                        log::error!("Failed to umount '{}': {}",
                            mountinfo.mount_point.display(), e);
                        return Err(Error::NixErrno(e))
                    }
                    exist = true;
                    break
                }
            }
        }
        Ok(self)
    }

    /// Root is expected
    pub(super) fn remove(&self) -> Result<&Self> {
        if self.0.exists() {
            log::info!("Removing '{}'...", self.0.display());
            self.umount_recursive()?;
            if let Err(e) = remove_dir_all(&self.0) {
                log::error!("Failed to remove '{}': {}",
                            self.0.display(), e);
                return Err(Error::IoError(e))
            }
        }
        Ok(self)
    }

    /// Root is expected
    pub(super) fn remove_all() -> Result<()> {
        Self(PathBuf::from("roots")).remove().and(Ok(()))
    }
}

impl Drop for MountedFolder {
    fn drop(&mut self) {
        if IdentityActual::as_root(||{
            self.remove().and(Ok(()))
        }).is_err() {
            log::error!("Failed to drop mounted folder '{}'", self.0.display());
        }
    }
}

pub(super) fn mount_checked<
    P1: ?Sized + NixPath,
    P2: ?Sized + NixPath,
    P3: ?Sized + NixPath,
    P4: ?Sized + NixPath,
    S1: Display,
    S2: Display
>(
    source: Option<&P1>,
    target: &P2,
    fstype: Option<&P3>,
    flags: MsFlags,
    data: Option<&P4>,
    source_human_readable: S1,
    target_human_readable: S2
) -> Result<()> {
    mount(source, target, fstype, flags, data).map_err(|e|{
        log::error!("Failed to mount '{}' to '{}': {}", 
            source_human_readable, target_human_readable, e);
        Error::NixErrno(e)
    })
}