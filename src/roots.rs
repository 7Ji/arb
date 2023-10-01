use std::{path::{PathBuf, Path}, fs::{remove_dir_all, create_dir_all}, ffi::{CString, OsStr, OsString}, os::unix::prelude::OsStrExt};

use super::identity::Identity;

/// The basic root, with bare-minimum packages installed
pub(super) struct Root {
    path: PathBuf,
    overlay: bool,
    complete: bool,
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

fn cstring_and_ptr_from_optional_osstr<S: AsRef<OsStr>> (osstr: Option<S>) 
    -> Result<(Option<CString>, *const i8), ()> 
{
    let cstring = match osstr {
        Some(osstr) => match CString::new(osstr.as_ref().as_bytes()) {
            Ok(osstr) => Some(osstr),
            Err(e) => {
                eprintln!(
                    "Failed to create c string from '{:?}': {}", 
                    osstr.as_ref(), e);
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

fn mount<S: AsRef<OsStr>>(
    src: Option<S>, target: S, fstype: Option<S>, 
    flags: libc::c_ulong, data: Option<S>
) 
    -> Result<(), ()> 
{
    let (src, src_ptr) = 
        cstring_and_ptr_from_optional_osstr(src)?;
    let target = match CString::new(target.as_ref().as_bytes()) {
        Ok(target) => target,
        Err(e) => {
            eprintln!("Failed to create c string from '{:?}' for target: {}", 
                target.as_ref(), e);
            return Err(())
        },
    };
    let (fstype, fstype_ptr) = 
        cstring_and_ptr_from_optional_osstr(fstype)?;
    let (data, data_ptr) = 
        cstring_and_ptr_from_optional_osstr(data)?;
    println!("Mounting {:?} to {:?}, type {:?}, data {:?}", 
        &src, &target, &fstype, &data);
    let r = unsafe {
        libc::mount(src_ptr, target.as_ptr(), fstype_ptr, 
            flags, data_ptr as *const libc::c_void)
    };
    if r != 0 {
        eprintln!("Failed to mount: {}", 
            std::io::Error::last_os_error());
        return Err(())
    }
    Ok(())
}

impl Root {
    fn new(name: &str, overlay: bool) -> Self {
        let mut path = PathBuf::from("roots");
        path.push(name);
        Self {
            path,
            overlay,
            complete: false
        }
    }

    fn umount_recursive(&self) -> Result<(), ()> {
        println!("Umounting '{}' recursively...", self.path.display());
        let absolute_path = match self.path.canonicalize() {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Failed to canoicalize path '{}': {}",
                    self.path.display(), e);
                return Err(())
            },
        };
        Identity::as_root(||{
            // as_root() actually forks, so we get the child's mountinfo
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
                        println!("Umounting {}", 
                            mountinfo.mount_point.display());
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
            Ok(())
        })?;
        println!("Umounted '{}'", self.path.display());
        Ok(())
    }

    fn remove(&self) -> Result<(), ()> {
        if self.path.exists() {
            println!("Removing '{}'...", self.path.display());
            self.umount_recursive()?;
            Identity::as_root(||{
                if let Err(e) = remove_dir_all(&self.path) {
                    eprintln!("Failed to remove '{}': {}", 
                                self.path.display(), e);
                    Err(())
                } else {
                    Ok(())
                }
            })
        } else {
            Ok(())
        }
    }

    /// The minimum mounts needed for execution, like how it's done by pacstrap
    fn mount(&self) -> Result<(), ()> {
        let subdirs = [
            "boot", "dev/pts", "dev/shm", "etc/pacman.d", "proc", "run", "sys", 
            "tmp", "var/cache/pacman/pkg", "var/lib/pacman", "var/log"];
        Identity::as_root(|| {
            for subdir in subdirs {
                let subdir = self.path.join(subdir);
                println!("Creating '{}'...", subdir.display());
                if let Err(e) = create_dir_all(&subdir) {
                    eprintln!("Failed to create dir '{}': {}", 
                        subdir.display(), e);
                    return Err(())
                }
            }
            mount(Some(self.path.as_os_str()),
                self.path.as_os_str(),
                None,
                libc::MS_BIND,
                None)?;
            mount(None,
                self.path.join("proc").as_os_str(),
                Some(&OsString::from("proc")),
                libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV,
                None)?;
            mount(None, 
                self.path.join("sys").as_os_str(),
                Some(&OsString::from("sysfs")),
                libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV | 
                    libc::MS_RDONLY,
                None)?;
            mount(None,
                self.path.join("dev").as_os_str(),
                Some(&OsString::from("devtmpfs")),
                libc::MS_NOSUID,
                Some(&OsString::from("mode=0755")))?;
            mount(None,
                self.path.join("dev/pts").as_os_str(),
                Some(&OsString::from("devpts")),
                libc::MS_NOSUID | libc::MS_NOEXEC,
                Some(&OsString::from("mode=0620,gid=5")))?;
            mount(None,
                self.path.join("dev/shm").as_os_str(),
                Some(&OsString::from("tmpfs")),
                libc::MS_NOSUID | libc::MS_NODEV,
                Some(&OsString::from("mode=1777")))?;
            mount(None,
                self.path.join("run").as_os_str(),
                Some(&OsString::from("tmpfs")),
                libc::MS_NOSUID | libc::MS_NODEV,
                Some(&OsString::from("mode=0755")))?;
            mount(None,
                self.path.join("tmp").as_os_str(),
                Some(&OsString::from("tmpfs")),
                libc::MS_STRICTATIME | libc::MS_NODEV | libc::MS_NOSUID,
                Some(&OsString::from("mode=1777")))?;
            Ok(())
        })
    }

    fn setup(&self) -> Result<(), ()> {
        assert!(self.overlay == false);
        let mut command = std::process::Command::new("/usr/bin/pacman");
        command
            .arg("-Sy")
            .arg("--root")
            .arg(self.path.canonicalize().or(Err(()))?)
            .arg("--noconfirm")
            .arg("base-devel");
        Identity::set_root_command(&mut command);
        command.spawn().unwrap().wait().unwrap();
        Ok(())
    }

    /// Create a base rootfs containing the minimum packages and user setup
    /// This should not be used directly for building packages
    /// Rather, you should later call .new_overlay()
    pub(crate) fn new_base() -> Result<Self, ()> {
        println!("Creating base root");
        let mut root = Self::new("base", false);
        root.remove()?;
        root.mount()?;
        root.setup()?;
        root.umount_recursive()?;
        root.complete = true;
        Ok(root)
    }

    /// Should only be called on root
    fn new_overlay(&self, name: &str, pkgs: &[&str]) -> Result<Self, ()> {
        if ! self.path.exists() {
            eprintln!("Cannot create overlay root from incomplete root");
            return Err(())
        }
        let mut root = Self::new(name, true);
        root.remove()?;
        root.complete = true;
        Ok(root)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        self.remove().expect("Failed to drop root")
    }
}