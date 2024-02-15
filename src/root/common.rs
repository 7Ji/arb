use std::{
        ffi::OsStr,
        fs::{
            copy,
            create_dir_all,
            remove_file,
        },
        path::{
            Path,
            PathBuf
        },
        process::Command,
    };

use nix::mount::MsFlags;

use crate::{
        error::{
            Error,
            Result
        },
        identity::IdentityActual,
        root::mount::mount_checked,
    };

pub(crate) trait CommonRoot {
    const BUILDER_DIRS: [&'static str; 3] = ["build", "pkgs", "sources"];
    // const MSFLAGS_PROC: MsFlags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV;

    fn path(&self) -> &Path;
    fn path_absolute(&self) -> Result<PathBuf> {
        let path = self.path();
        path.canonicalize().map_err(|e|{
            log::error!("Failed to caonoicalize path '{}': {}", path.display(), e);
            Error::IoError(e)
        })
    }
    fn db_path(&self) -> PathBuf {
        self.path().join("var/lib/pacman")
    }
    // fn fresh_install() -> bool;
    /// Root is expected
    fn base_layout(&self) -> Result<&Self> {
        for subdir in [
            "boot", "dev/pts", "dev/shm", "etc/pacman.d", "proc", "run", "sys",
            "tmp", "var/cache/pacman/pkg", "var/lib/pacman", "var/log"]
        {
            let subdir = self.path().join(subdir);
            // log::info!("Creating '{}'...", subdir.display());
            if let Err(e) = create_dir_all(&subdir) {
                log::error!("Failed to create dir '{}': {}",
                    subdir.display(), e);
                return Err(Error::IoError(e))
            }
        }
        Ok(self)
    }

    fn mount_proc(&self) -> Result<&Self> {
        let path_proc = self.path().join("proc");
        mount_checked(Some("proc"),
            &path_proc,
            Some("proc"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
            None::<&str>,
            "proc",
            path_proc.display()
        ).and(Ok(self))
    }

    fn mount_sys(&self) -> Result<&Self> {
        let path_sys = self.path().join("sys");
        mount_checked(Some("sys"),
            &path_sys,
            Some("sysfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV |
                MsFlags::MS_RDONLY,
            None::<&str>,
            "sys",
            path_sys.display()
        ).and(Ok(self))
    }

    fn mount_dev(&self) -> Result<&Self> {
        let path_dev = self.path().join("dev");
        mount_checked(Some("udev"),
            &path_dev,
            Some("devtmpfs"),
            MsFlags::MS_NOSUID,
            Some("mode=0755"),
            "dev",
            path_dev.display()
        ).and(Ok(self))
    }

    fn mount_devpts(&self) -> Result<&Self> {
        let path_devpts = self.path().join("dev/pts");
        mount_checked(Some("devpts"),
            &path_devpts,
            Some("devpts"),
            MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
            Some("mode=0620,gid=5"),
            "devpts",
            path_devpts.display()
        ).and(Ok(self)) 
    }

    fn mount_devshm(&self) -> Result<&Self> {
        let path_devshm = self.path().join("dev/shm");
        mount_checked(Some("shm"),
            &path_devshm,
            Some("tmpfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
            Some("mode=1777"),
            "tmpfs",
            path_devshm.display()
        ).and(Ok(self))
    }

    fn mount_run(&self) -> Result<&Self> {
        let path_run = self.path().join("run");
        mount_checked(Some("run"),
            &path_run,
            Some("tmpfs"),
            MsFlags::MS_NOSUID | MsFlags::MS_NODEV,
            Some("mode=0755"),
            "tmpfs",
            path_run.display()
        ).and(Ok(self))
    }

    fn mount_tmp(&self) -> Result<&Self> {
        let path_tmp = self.path().join("tmp");
        mount_checked(Some("tmp"),
            &path_tmp,
            Some("tmpfs"),
            MsFlags::MS_STRICTATIME | MsFlags::MS_NODEV | MsFlags::MS_NOSUID,
            Some("mode=1777"),
            "tmpfs",
            path_tmp.display()
        ).and(Ok(self))
    }

    /// The minimum mounts needed for execution, like how it's done by pacstrap.
    /// Root is expected.
    fn base_mounts(&self) -> Result<&Self> {
        self.mount_proc()?
            .mount_sys()?
            .mount_dev()?
            .mount_devpts()?
            .mount_devshm()?
            .mount_run()?
            .mount_tmp()
    }

    // Todo: split out common wait child parts
    fn refresh_dbs(&self) -> Result<&Self> {
        crate::child::output_and_check(
            crate::logfile::LogFile::new(
                crate::logfile::LogType::Pacman, "refresh DB")?
                .set_command(
                    Command::new("/usr/bin/pacman")
                    .env("LANG", "C")
                    .arg("-Sy")
                    .arg("--root")
                    .arg(self.path_absolute()?)
                )?,
            "refresh pacman DB").and(Ok(self))
    }

    fn install_pkgs<I, S>(&self, pkgs: I)
        -> Result<&Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new("/usr/bin/pacman");
        command
            .env("LANG", "C")
            .arg("-S")
            .arg("--root")
            .arg(self.path())
            .arg("--dbpath")
            .arg(self.db_path())
            .arg("--noconfirm")
            .arg("--needed");
        let mut has_pkg = false;
        for pkg in pkgs {
            has_pkg = true;
            command.arg(pkg);
        }
        if ! has_pkg {
            return Ok(self)
        }
        crate::child::output_and_check(
            crate::logfile::LogFile::new(
                crate::logfile::LogType::Pacman, "install packages")?
                .set_command(&mut command)?,
            "install pkgs").and(Ok(self))
    }

    fn resolv(&self) -> Result<&Self> {
        let resolv = self.path().join("etc/resolv.conf");
        if resolv.exists() {
            if let Err(e) = remove_file(&resolv) {
                log::error!("Failed to remove resolv from root: {}", e);
                return Err(Error::IoError(e))
            }
        }
        Self::copy_file("/etc/resolv.conf", &resolv)?;
        Ok(self)
    }

    fn copy_file<P: AsRef<Path>, Q: AsRef<Path>>(source: P, target: Q)
        -> Result<()>
    {
        if let Err(e) = copy(&source, &target) {
            log::error!("Failed to copy from '{}' to '{}': {}",
                source.as_ref().display(), target.as_ref().display(), e);
            Err(Error::IoError(e))
        } else {
            Ok(())
        }
    }

    fn copy_file_same<P: AsRef<Path>>(&self, suffix: P) -> Result<&Self> {
        let source = PathBuf::from("/").join(&suffix);
        let target = self.path().join(&suffix);
        Self::copy_file(source, target).and(Ok(self))
    }

    fn home(&self, actual_identity: &IdentityActual) -> Result<PathBuf> {
        Ok(self.path().join(actual_identity.home_no_root()?))
    }

    fn builder_raw(root_path: &Path, actual_identity: &IdentityActual)
        -> Result<PathBuf>
    {
        Ok(root_path.join(actual_identity.cwd_no_root()?))
    }

    fn builder(&self, actual_identity: &IdentityActual) -> Result<PathBuf> {
        Self::builder_raw(self.path(), actual_identity)
    }
}
