use std::{path::{PathBuf, Path}, fs::{remove_dir_all, create_dir_all}, ffi::CString, os::unix::prelude::OsStrExt, process::Command};

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
    fn path(&self) -> &Path;
    fn db_path(&self) -> PathBuf {
        self.path().join("var/lib/pacman")
    }
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

    fn remove(&self) -> Result<&Self, ()> {
        match self.0.remove() {
            Ok(_) => Ok(self),
            Err(_) => Err(()),
        }
    }

    fn umount_recursive(&self) -> Result<&Self, ()> {
        match self.0.umount_recursive() {
            Ok(_) => Ok(self),
            Err(_) => Err(()),
        }
    }

    fn setup(&self) -> Result<&Self, ()> {
        let mut command = Command::new("/usr/bin/pacman");
        command
            .env("LANG", "C")
            .arg("-Sy")
            .arg("--root")
            .arg(self.path().canonicalize().or(Err(()))?)
            .arg("--noconfirm")
            .arg("base-devel");
        Identity::set_root_command(&mut command);
        if command.spawn().unwrap().wait().unwrap().code().unwrap() != 0 {
            eprintln!("Install command failed to execute correctly");
            return Err(())
        }
        Ok(self)
    }

    /// Create a base rootfs containing the minimum packages and user setup
    /// This should not be used directly for building packages
    pub(crate) fn new() -> Result<Self, ()> {
        println!("Creating base chroot");
        let root = Self(MountedFolder(PathBuf::from("roots/base")));
        Identity::as_root(||{
            root.remove()?
                .base_layout()?
                .bind_self()?
                .base_mounts()?
                .setup()?
                .umount_recursive()?;
            Ok(())
        })?;
        Ok(root)
    }
}

impl CommonRoot for BaseRoot {
    fn path(&self) -> &Path {
        self.0.0.as_path()
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

    fn pkgs(&self, pkgs: &Vec<String>) -> Result<&Self, ()> {
        if pkgs.len() == 0 {
            return Ok(self)
        }
        let mut command = Command::new("/usr/bin/pacman");
        command
            .env("LANG", "C")
            .arg("-S")
            .arg("--root")
            .arg(self.path().canonicalize().or(Err(()))?)
            .arg("--needed")
            .arg("--noconfirm")
            .args(pkgs);
        Identity::set_root_command(&mut command);
        command.spawn().unwrap().wait().unwrap();
        Ok(self)
    }

    /// Different from base, overlay would have upper, work, and merged.
    /// Note that the pkgs here can only come from repos, not as raw pkg files.
    pub(crate) fn new(name: &str, pkgs: &Vec<String>) -> Result<Self, ()> 
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
                .pkgs(pkgs)?;
            Ok(())
        })?;
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