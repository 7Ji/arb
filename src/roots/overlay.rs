use std::{path::{PathBuf, Path}, fs::{remove_dir_all, create_dir_all, create_dir}, ffi::OsStr};

use crate::identity::{IdentityActual, ForkedChild, Identity};

use super::{mount::{MountedFolder, mount}, common::CommonRoot};


pub(crate) struct OverlayRoot {
    parent: PathBuf,
    upper: PathBuf,
    work: PathBuf,
    merged: MountedFolder,
}

impl OverlayRoot {
    fn remove(&self) -> Result<&Self, ()> {
        if self.merged.remove().is_err() {
            return Err(())
        }
        if self.parent.exists() {
            if let Err(e) = remove_dir_all(&self.parent) {
                eprintln!("Failed to remove '{}': {}", 
                            self.parent.display(), e);
                return Err(())
            }
        }
        Ok(self)
    }

    fn overlay(&self) -> Result<&Self, ()> {
        for dir in [&self.upper, &self.work, &self.merged.0] {
            create_dir_all(dir).or(Err(()))?
        }
        mount(Some("overlay"),
            &self.merged.0,
            Some("overlay"),
            0,
            Some(&format!(
                "lowerdir=roots/base,upperdir={},workdir={}", 
                self.upper.display(), self.work.display())))?;
        Ok(self)
    }

    fn bind_builder(&self, actual_identity: &IdentityActual) -> Result<&Self, ()> {
        let builder = self.builder(actual_identity)?;
        for dir in Self::BUILDER_DIRS {
            mount(Some(dir),
                &builder.join(dir),
                None,
                libc::MS_BIND,
                None)?;
        }
        Ok(self)
    }

    fn bind_homedirs<I, S>(&self, actual_identity: &IdentityActual, home_dirs: I) 
        -> Result<&Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
        let host_home_path = actual_identity.home_path();
        let mut host_home_string = actual_identity.home_str().to_string();
        host_home_string.push('/');
        let chroot_home = self.home(actual_identity)?;
        for dir in home_dirs {
            let host_dir = host_home_path.join(dir.as_ref());
            if ! host_dir.exists() {
                continue
            }
            let chroot_dir = chroot_home.join(dir.as_ref());
            create_dir(&chroot_dir).or_else(|e|{
                eprintln!("Failed to create chroot dir: {}", e);
                Err(())
            })?;
            let mut host_dir_string = host_home_string.clone();
            host_dir_string.push_str(dir.as_ref());
            mount(Some(&host_dir_string),
                &chroot_dir,
                None,
                libc::MS_BIND,
                None)?;
        }
        Ok(self)
    }

    fn new_no_init(name: &str) -> Self {
        let parent = PathBuf::from(format!("roots/overlay-{}", name));
        let upper = parent.join("upper");
        let work = parent.join("work");
        let merged = MountedFolder(parent.join("merged"));
        Self {
            parent,
            upper,
            work,
            merged,
        }
    }

    fn new_child<I, S, I2, S2>(
        name: &str, actual_identity: &IdentityActual, pkgs: I, home_dirs: I2,
        nonet: bool
    ) -> Result<(Self, ForkedChild), ()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        I2: IntoIterator<Item = S2>,
        S2: AsRef<str>
    {
        println!("Creating overlay chroot '{}'", name);
        let root = Self::new_no_init(name);
        let child = IdentityActual::as_root_child(||{
            root.remove()?
                .overlay()?
                .base_mounts()?
                .install_pkgs(pkgs)?
                .bind_builder(actual_identity)?
                .bind_homedirs(actual_identity, home_dirs)?;
            if ! nonet {
                root.resolv()?;
            }
            Ok(())
        })?;
        println!("Forked child to create overlay chroot '{}'", name);
        Ok((root, child))
    }

    /// Different from base, overlay would have upper, work, and merged.
    /// Note that the pkgs here can only come from repos, not as raw pkg files.
    pub(crate) fn _new<I, S, I2, S2>(
        name: &str, actual_identity: &IdentityActual, pkgs: I, home_dirs: I2,
        nonet: bool
    ) -> Result<Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        I2: IntoIterator<Item = S2>,
        S2: AsRef<str>
    {
        println!("Creating overlay chroot '{}'", name);
        let root = Self::new_no_init(name);
        IdentityActual::as_root(||{
            root.remove()?
                .overlay()?
                .base_mounts()?
                .install_pkgs(pkgs)?
                .bind_builder(actual_identity)?
                .bind_homedirs(actual_identity, home_dirs)?;
            if ! nonet {
                root.resolv()?;
            }
            Ok(())
        })?;
        println!("Created overlay chroot '{}'", name);
        Ok(root)
    }

    pub(crate) fn get_root_no_init(name: &str) 
        -> PathBuf
    {
        PathBuf::from(format!("roots/overlay-{}/merged", name))
    }
}

impl CommonRoot for OverlayRoot {
    fn path(&self) -> &Path {
        self.merged.0.as_path()
    }
}
    

impl Drop for OverlayRoot {
    fn drop(&mut self) {
        if IdentityActual::as_root(||{
            self.remove().and(Ok(()))
        }).is_err() {
            eprintln!("Failed to drop overlay root '{}'", self.parent.display())
        }
    }
}


pub(crate) struct BootstrappingOverlayRoot {
    root: OverlayRoot,
    child: ForkedChild,
    status: Option<Result<(), ()>>,
}


impl BootstrappingOverlayRoot {
    pub(crate) fn new<I, S, I2, S2>(
        name: &str, actual_identity: &IdentityActual, pkgs: I, home_dirs: I2,
        nonet: bool
    ) -> Result<Self, ()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        I2: IntoIterator<Item = S2>,
        S2: AsRef<str>
    {
        let (root, child) = OverlayRoot::new_child(
            name, actual_identity, pkgs, home_dirs, nonet)?;
        Ok(Self {
            root,
            child,
            status: None
        })
    }

    pub(crate) fn wait_noop(&mut self) -> Result<Option<Result<(), ()>>, ()>{
        assert!(self.status.is_none());
        let r = self.child.wait_noop();
        if let Ok(r) = r {
            if let Some(r) = r {
                self.status = Some(r)
            }
        }
        r
    }

    pub(crate) fn wait(self) -> Result<OverlayRoot, ()> {
        let status = match self.status {
            Some(status) => status,
            None => self.child.wait(),
        };
        match status {
            Ok(_) => Ok(self.root),
            Err(_) => Err(()),
        }
    }
}