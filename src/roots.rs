use std::{
        ffi::{
            CString, 
            OsStr,
        },
        fs::{
            remove_dir_all, 
            create_dir_all, 
            copy, 
            create_dir, 
            remove_file
        },
        os::unix::prelude::OsStrExt,
        path::{
            PathBuf, 
            Path,
        }, 
        process::{Command, Stdio}
    };


use crate::identity::ForkedChild;

use super::identity::Identity;

#[derive(Clone)]
struct MountedFolder (PathBuf);

/// The basic root, with bare-minimum packages installed
#[derive(Clone)]
pub(super) struct BaseRoot (MountedFolder);

pub(super) struct OverlayRoot {
    parent: PathBuf,
    upper: PathBuf,
    work: PathBuf,
    merged: MountedFolder,
}

pub(super) struct BootstrappingOverlayRoot {
    root: OverlayRoot,
    child: ForkedChild,
    status: Option<Result<(), ()>>,
}

fn cstring_from_path(path: &Path) -> Result<CString, ()> {
    match CString::new(path.as_os_str().as_bytes()) 
    {
        Ok(path) => Ok(path),
        Err(e) => {
            eprintln!("Failed to create c string from path '{}': {}",
                path.display(), e);
            Err(())
        },
    }
}

fn cstring_and_ptr_from_optional_str<S: AsRef<str>> (opstr: Option<S>) 
    -> Result<(Option<CString>, *const libc::c_char), ()> 
{
    let cstring = match opstr {
        Some(opstr) => match CString::new(opstr.as_ref().as_bytes()) {
            Ok(opstr) => Some(opstr),
            Err(e) => {
                eprintln!(
                    "Failed to create c string from '{:?}': {}", 
                    opstr.as_ref(), e);
                return Err(())
            },
        },
        None => None,
    };
    let ptr = match &cstring {
        Some(cstring) => cstring.as_ptr(),
        None => std::ptr::null(),
    };
    Ok((cstring, ptr))
}

fn mount(
    src: Option<&str>, target: &Path, fstype: Option<&str>,
    flags: libc::c_ulong, data: Option<&str>
) 
    -> Result<(), ()> 
{
    let (_src, src_ptr) = 
        cstring_and_ptr_from_optional_str(src)?;
    let (_fstype, fstype_ptr) = 
        cstring_and_ptr_from_optional_str(fstype)?;
    let (_data, data_ptr) = 
        cstring_and_ptr_from_optional_str(data)?;
    let target = 
        CString::new(target.as_os_str().as_bytes()).or(Err(()))?;
    let r = unsafe {
        libc::mount(src_ptr, target.as_ptr(), fstype_ptr, flags, 
            data_ptr as *const libc::c_void)
    };
    if r != 0 {
        eprintln!("Failed to mount {:?} to {:?}, fstype {:?}, flags {:?}, \
                    data {:?}: {}",
                    src, target, fstype, flags, data, 
                    std::io::Error::last_os_error());
        return Err(())
    }
    Ok(())
}

impl MountedFolder {
    /// Umount any folder starting from the path.
    /// Root is expected
    fn umount_recursive(&self) -> Result<&Self, ()> {
        println!("Umounting '{}' recursively...", self.0.display());
        let absolute_path = match self.0.canonicalize() {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Failed to canoicalize path '{}': {}",
                    self.0.display(), e);
                return Err(())
            },
        };
        let process = match procfs::process::Process::myself() {
            Ok(process) => process,
            Err(e) => {
                eprintln!("Failed to get myself: {}", e);
                return Err(())
            },
        };
        let mut exist = true;
        while exist {
            let mountinfos = match process.mountinfo() {
                Ok(mountinfos) => mountinfos,
                Err(e) => {
                    eprintln!("Failed to get mountinfos: {}", e);
                    return Err(())
                },
            };
            exist = false;
            for mountinfo in mountinfos.iter().rev() {
                if mountinfo.mount_point.starts_with(&absolute_path) {
                    // println!("Umounting {}", 
                    //     mountinfo.mount_point.display());
                    let path = cstring_from_path(
                            &mountinfo.mount_point)?;
                    let r = unsafe {
                        libc::umount(path.as_ptr())
                    };
                    if r != 0 {
                        eprintln!("Failed to umount '{}': {}",
                            mountinfo.mount_point.display(), 
                            std::io::Error::last_os_error());
                        return Err(())
                    }
                    exist = true;
                    break
                }
            }
        }
        // println!("Umounted '{}'", self.0.display());
        Ok(self)
    }

    /// Root is expected
    fn remove(&self) -> Result<&Self, ()> {
        if self.0.exists() {
            println!("Removing '{}'...", self.0.display());
            self.umount_recursive()?;
            if let Err(e) = remove_dir_all(&self.0) {
                eprintln!("Failed to remove '{}': {}", 
                            self.0.display(), e);
                return Err(())
            }
        }
        Ok(self)
    }

    /// Root is expected
    fn remove_all() -> Result<(), ()> {
        // Force remove method to run on roots
        let _ = Self(PathBuf::from("roots"));
        Ok(())
    }
}

impl Drop for MountedFolder {
    fn drop(&mut self) {
        if Identity::as_root(||{
            self.remove().and(Ok(()))
        }).is_err() {
            eprintln!("Failed to drop mounted folder '{}'", self.0.display());
        }
    }
}

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
            // println!("Creating '{}'...", subdir.display());
            if let Err(e) = create_dir_all(&subdir) {
                eprintln!("Failed to create dir '{}': {}", 
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
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV,
            None)?;
        mount(Some("sys"),
            &self.path().join("sys"),
            Some("sysfs"),
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV | 
                libc::MS_RDONLY,
            None)?;
        mount(Some("udev"),
            &self.path().join("dev"),
            Some("devtmpfs"),
            libc::MS_NOSUID,
            Some("mode=0755"))?;
        mount(Some("devpts"),
            &self.path().join("dev/pts"),
            Some("devpts"),
            libc::MS_NOSUID | libc::MS_NOEXEC,
            Some("mode=0620,gid=5"))?;
        mount(Some("shm"),
            &self.path().join("dev/shm"),
            Some("tmpfs"),
            libc::MS_NOSUID | libc::MS_NODEV,
            Some("mode=1777"))?;
        mount(Some("run"),
            &self.path().join("run"),
            Some("tmpfs"),
            libc::MS_NOSUID | libc::MS_NODEV,
            Some("mode=0755"))?;
        mount(Some("tmp"),
            &self.path().join("tmp"),
            Some("tmpfs"),
            libc::MS_STRICTATIME | libc::MS_NODEV | libc::MS_NOSUID,
            Some("mode=1777"))?;
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
                eprintln!("Failed to spawn child to refresh DB: {}", e);
                Err(())
            })?
            .status
            .code()
            .ok_or_else(||{
                eprintln!("Failed to get code from child to refresh DB");
            })?;
        if r != 0 {
            eprintln!("Failed to execute refresh command, return: {}", r);
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
            .arg(self.path().canonicalize().or(Err(()))?)
            .arg("--noconfirm")
            .arg("--needed")
            .args(pkgs)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .or_else(|e|{
                eprintln!("Failed to spawn child to install pkgs: {}", e);
                Err(())
            })?
            .status
            .code()
            .ok_or_else(||{
                eprintln!(
                    "Failed to get return code from child to install pkgs");
            })?;
        if r != 0 {
            eprintln!("Failed to execute install command, return: {}", r);
            return Err(())
        }
        Ok(self)
    }

    fn resolv(&self) -> Result<&Self, ()> {
        let resolv = self.path().join("etc/resolv.conf");
        if resolv.exists() {
            remove_file(&resolv).or_else(|e|{
                eprintln!("Failed to remove resolv from root: {}", e);
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
                eprintln!("Failed to copy from '{}' to '{}': {}",
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

    fn home(&self, actual_identity: &Identity) -> Result<PathBuf, ()> {
        let home: PathBuf = actual_identity.home()?;
        let home_suffix = home.strip_prefix("/").or_else(
            |e| {
                eprintln!("Failed to strip home prefix: {}", e);
                Err(())
            })?;
        Ok(self.path().join(home_suffix))
    }

    fn builder(&self, actual_identity: &Identity) -> Result<PathBuf, ()> {
        let cwd = actual_identity.cwd()?;
        let suffix = cwd.strip_prefix("/").or_else(
            |e|{
                eprintln!("Failed to strip suffix from cwd: {}", e);
                Err(())
            })?;
        Ok(self.path().join(suffix))
    }
}

impl BaseRoot {
    pub(crate) fn as_str(&self) -> &str {
        "roots/base"
    }

    fn path(&self) -> &Path {
        &self.0.0
    }

    /// Root is expected
    fn bind_self(&self) -> Result<&Self, ()> {
        mount(Some("roots/base"),
                self.path(),
                None,
                libc::MS_BIND,
                None)?;
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
    fn create_home(&self, actual_identity: &Identity) -> Result<&Self, ()> {
        // std::thread::sleep(std::time::Duration::from_secs(100));
        Identity::run_chroot_command(
            Command::new("/usr/bin/mkhomedir_helper")
                .arg(actual_identity.user()?),
            self.path())?;
        Ok(self)
    }

    /// Root is expected
    fn setup(&self, actual_identity: &Identity) -> Result<&Self, ()> {
        eprintln!("Finishing base root setup");
        let builder = self.builder(actual_identity)?;
        self.copy_file_same("etc/passwd")?
            .copy_file_same("etc/group")?
            .copy_file_same("etc/shadow")?
            .copy_file_same("etc/makepkg.conf")?
            .create_home(actual_identity)?;
        create_dir_all(&builder)
            .or_else(|e|{
                eprintln!("Failed to create chroot builder dir: {}", e);
                Err(())
            })?;
        for dir in Self::BUILDER_DIRS {
            create_dir(builder.join(dir))
                .or_else(|e|{
                    eprintln!("Failed to create chroot builder dir: {}", e);
                    Err(())
                })?;
        }
        eprintln!("Finished base root setup");
        Ok(self)
    }

    pub(crate) fn db_only() -> Result<Self, ()> {
        Identity::as_root(||MountedFolder::remove_all())?;
        println!("Creating base chroot (DB only)");
        let root = Self(MountedFolder(PathBuf::from("roots/base")));
        Identity::as_root(||{
            root.remove()?
                .base_layout()?
                .bind_self()?
                .base_mounts()?
                .refresh_dbs()?;
            Ok(())
        })?;
        println!("Created base chroot (DB only)");
        Ok(root)
    }

    /// Create a base rootfs containing the minimum packages and user setup
    /// This should not be used directly for building packages
    pub(crate) fn _new<I, S>(actual_identity: &Identity, pkgs: I) 
        -> Result<Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>
    {
        Identity::as_root(||MountedFolder::remove_all())?;
        println!("Creating base chroot");
        let root = Self(MountedFolder(PathBuf::from("roots/base")));
        Identity::as_root(||{
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
        println!("Created base chroot");
        Ok(root)
    }

    /// Finish a DB-only base root
    pub(crate) fn finish<I, S>(&self, actual_identity: &Identity, pkgs: I) 
        -> Result<&Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>
    {
        Identity::as_root(||{
            self.install_pkgs(pkgs)?
                .setup(actual_identity)?
                .umount_recursive()?;
            Ok(())
        })?;
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
        let _ = MountedFolder::remove_all();
    }
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

    fn bind_builder(&self, actual_identity: &Identity) -> Result<&Self, ()> {
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

    fn bind_homedirs<I, S>(&self, actual_identity: &Identity, home_dirs: I) 
        -> Result<&Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
        let host_home = actual_identity.home()?;
        let mut host_home_string = actual_identity.home_string()?;
        host_home_string.push('/');
        let chroot_home = self.home(actual_identity)?;
        for dir in home_dirs {
            let host_dir = host_home.join(dir.as_ref());
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

    fn new_child<I, S, I2, S2>(
        name: &str, actual_identity: &Identity, pkgs: I, home_dirs: I2,
        nonet: bool
    ) -> Result<(Self, ForkedChild), ()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        I2: IntoIterator<Item = S2>,
        S2: AsRef<str>
    {
        println!("Creating overlay chroot '{}'", name);
        let parent = PathBuf::from(format!("roots/overlay-{}", name));
        let upper = parent.join("upper");
        let work = parent.join("work");
        let merged = MountedFolder(parent.join("merged"));
        let root = Self {
            parent,
            upper,
            work,
            merged,
        };
        let child = Identity::as_root_child(||{
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
    pub(crate) fn new<I, S, I2, S2>(
        name: &str, actual_identity: &Identity, pkgs: I, home_dirs: I2,
        nonet: bool
    ) -> Result<Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
        I2: IntoIterator<Item = S2>,
        S2: AsRef<str>
    {
        println!("Creating overlay chroot '{}'", name);
        let parent = PathBuf::from(format!("roots/overlay-{}", name));
        let upper = parent.join("upper");
        let work = parent.join("work");
        let merged = MountedFolder(parent.join("merged"));
        let root = Self {
            parent,
            upper,
            work,
            merged,
        };
        Identity::as_root(||{
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
}

impl CommonRoot for OverlayRoot {
    fn path(&self) -> &Path {
        self.merged.0.as_path()
    }
}
    

impl Drop for OverlayRoot {
    fn drop(&mut self) {
        if Identity::as_root(||{
            self.remove().and(Ok(()))
        }).is_err() {
            eprintln!("Failed to drop overlay root '{}'", self.parent.display())
        }
    }
}

impl BootstrappingOverlayRoot {
    pub(super) fn new<I, S, I2, S2>(
        name: &str, actual_identity: &Identity, pkgs: I, home_dirs: I2,
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

    pub(super) fn wait_noop(&mut self) -> Result<Option<Result<(), ()>>, ()>{
        assert!(self.status.is_none());
        let r = self.child.wait_noop();
        if let Ok(r) = r {
            if let Some(r) = r {
                self.status = Some(r)
            }
        }
        r
    }

    pub(super) fn wait(self) -> Result<OverlayRoot, ()> {
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