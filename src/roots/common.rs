use std::{path::{Path, PathBuf}, fs::{create_dir_all, remove_file, copy}, ffi::OsStr, process::{Command, Stdio}};

use crate::identity::IdentityActual;

use nix::mount::{mount, MsFlags};

pub(crate) trait CommonRoot {
    const BUILDER_DIRS: [&'static str; 3] = ["build", "pkgs", "sources"];

    fn path(&self) -> &Path;
    fn db_path(&self) -> PathBuf {
        self.path().join("var/lib/pacman")
    }
    // fn fresh_install() -> bool;
    /// Root is expected
    fn base_layout(&self) -> Result<&Self, ()> {
        for subdir in [
            "boot", "dev/pts", "dev/shm", "etc/pacman.d", "proc", "run", "sys",
            "tmp", "var/cache/pacman/pkg", "var/lib/pacman", "var/log"]
        {
            let subdir = self.path().join(subdir);
            // log::info!("Creating '{}'...", subdir.display());
            if let Err(e) = create_dir_all(&subdir) {
                log::error!("Failed to create dir '{}': {}",
                    subdir.display(), e);
                return Err(())
            }
        }
        Ok(self)
    }

    /// The minimum mounts needed for execution, like how it's done by pacstrap.
    /// Root is expected.
    fn base_mounts(&self) -> Result<&Self, ()> {
        mount(Some("proc"),
            &self.path().join("proc"),
            Some("proc"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
            None::<&str>
        ).map_err(|e|
            log::error!("Failed to mount proc for '{}': {}",
            self.path().display(), e))?;
        mount(Some("sys"),
            &self.path().join("sys"),
            Some("sysfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV |
                MsFlags::MS_RDONLY,
            None::<&str>
        ).map_err(|e|
            log::error!("Failed to mount sys for '{}': {}",
            self.path().display(), e))?;
        mount(Some("udev"),
            &self.path().join("dev"),
            Some("devtmpfs"),
            MsFlags::MS_NOSUID,
            Some("mode=0755")
        ).map_err(|e|
            log::error!("Failed to mount udev for '{}': {}",
            self.path().display(), e))?;
        mount(Some("devpts"),
            &self.path().join("dev/pts"),
            Some("devpts"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
            Some("mode=0620,gid=5")
        ).map_err(|e|
            log::error!("Failed to mount devpts for '{}': {}",
            self.path().display(), e))?;
        mount(Some("shm"),
            &self.path().join("dev/shm"),
            Some("tmpfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
            Some("mode=1777")
        ).map_err(|e|
            log::error!("Failed to mount shm for '{}': {}",
            self.path().display(), e))?;
        mount(Some("run"),
            &self.path().join("run"),
            Some("tmpfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
            Some("mode=0755")
        ).map_err(|e|
            log::error!("Failed to mount run for '{}': {}",
            self.path().display(), e))?;
        mount(Some("tmp"),
            &self.path().join("tmp"),
            Some("tmpfs"),
            MsFlags::MS_STRICTATIME | MsFlags::MS_NODEV | MsFlags::MS_NOSUID,
            Some("mode=1777")
        ).map_err(|e|
            log::error!("Failed to mount tmp for '{}': {}",
            self.path().display(), e))?;
        Ok(self)
    }

    // Todo: split out common wait child parts
    fn refresh_dbs(&self) -> Result<&Self, ()> {
        let r = Command::new("/usr/bin/pacman")
            .env("LANG", "C")
            .arg("-Sy")
            .arg("--root")
            .arg(self.path().canonicalize().or(Err(()))?)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .or_else(|e| {
                log::error!("Failed to spawn child to refresh DB: {}", e);
                Err(())
            })?
            .status
            .code()
            .ok_or_else(||{
                log::error!("Failed to get code from child to refresh DB");
            })?;
        if r != 0 {
            log::error!("Failed to execute refresh command, return: {}", r);
            return Err(())
        }
        Ok(self)
    }

    fn install_pkgs<I, S>(&self, pkgs: I)
        -> Result<&Self, ()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let pkgs: Vec<S> = pkgs.into_iter().collect();
        if pkgs.len() == 0 {
            return Ok(self)
        }
        let r = Command::new("/usr/bin/pacman")
            .env("LANG", "C")
            .arg("-S")
            .arg("--root")
            .arg(self.path())
            .arg("--dbpath")
            .arg(self.db_path())
            .arg("--noconfirm")
            .arg("--needed")
            .args(pkgs)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .or_else(|e|{
                log::error!("Failed to spawn child to install pkgs: {}", e);
                Err(())
            })?
            .status
            .code()
            .ok_or_else(||{
                log::error!(
                    "Failed to get return code from child to install pkgs");
            })?;
        if r != 0 {
            log::error!("Failed to execute install command, return: {}", r);
            return Err(())
        }
        Ok(self)
    }

    fn resolv(&self) -> Result<&Self, ()> {
        let resolv = self.path().join("etc/resolv.conf");
        if resolv.exists() {
            remove_file(&resolv).or_else(|e|{
                log::error!("Failed to remove resolv from root: {}", e);
                Err(())
            })?;
        }
        Self::copy_file("/etc/resolv.conf", &resolv)?;
        Ok(self)
    }

    fn copy_file<P: AsRef<Path>, Q: AsRef<Path>>(source: P, target: Q)
        -> Result<(), ()>
    {
        match copy(&source, &target) {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Failed to copy from '{}' to '{}': {}",
                    source.as_ref().display(), target.as_ref().display(), e);
                Err(())
            },
        }

    }

    fn copy_file_same<P: AsRef<Path>>(&self, suffix: P) -> Result<&Self, ()> {
        let source = PathBuf::from("/").join(&suffix);
        let target = self.path().join(&suffix);
        Self::copy_file(source, target).and(Ok(self))
    }

    fn home(&self, actual_identity: &IdentityActual) -> Result<PathBuf, ()> {
        let home_suffix = actual_identity.home_path()
        .strip_prefix("/").or_else(
            |e| {
                log::error!("Failed to strip home prefix: {}", e);
                Err(())
            })?;
        Ok(self.path().join(home_suffix))
    }

    fn builder_raw(root_path: &Path, actual_identity: &IdentityActual)
        -> Result<PathBuf, ()>
    {
        let suffix = actual_identity.cwd().strip_prefix("/").or_else(
            |e|{
                log::error!("Failed to strip suffix from cwd: {}", e);
                Err(())
            })?;
        Ok(root_path.join(suffix))
    }

    fn builder(&self, actual_identity: &IdentityActual) -> Result<PathBuf, ()> {
        Self::builder_raw(self.path(), actual_identity)
    }
}
