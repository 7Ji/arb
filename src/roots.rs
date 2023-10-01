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

fn mount(
    src: &OsStr, target: &OsStr, fstype: Option<&OsStr>, flags: libc::c_ulong
) 
    -> Result<(), ()> 
{
    let src = match CString::new(src.as_bytes()) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("Failed to create c string from '{:?}' for source: {}", 
                src, e);
            return Err(())
        },
    };
    let target = match CString::new(target.as_bytes()) {
        Ok(target) => target,
        Err(e) => {
            eprintln!("Failed to create c string from '{:?}' for target: {}", 
                target, e);
            return Err(())
        },
    };
    let fstype = match fstype {
        Some(fstype) => match CString::new(fstype.as_bytes()) {
            Ok(fstype) => Some(fstype),
            Err(e) => {
                eprintln!(
                    "Failed to create c string from '{:?}' for target: {}", 
                    src, e);
                return Err(())
            },
        },
        None => None,
    };
    println!("Mounting {:?} to {:?}, type {:?}", &src, &target, &fstype);
    let fstype_ptr = match fstype {
        Some(fstype) => fstype.as_ptr(),
        None => std::ptr::null(),
    };
    let r = unsafe {
        libc::mount(src.as_ptr(), target.as_ptr(), fstype_ptr, 
            flags, std::ptr::null())
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
        let process = match procfs::process::Process::myself() {
            Ok(process) => process,
            Err(e) => {
                eprintln!("Failed to get myself: {}", e);
                return Err(())
            },
        };
        let mut mountinfos = match process.mountinfo() {
            Ok(mountinfos) => mountinfos,
            Err(e) => {
                eprintln!("Failed to get mountinfos: {}", e);
                return Err(())
            },
        };
        mountinfos.retain(|mountinfo| {
            mountinfo.mount_point.starts_with(&absolute_path)
        });
        Identity::as_root(||{
            for mountinfo in mountinfos.iter().rev() {
                println!("Umounting {}", mountinfo.mount_point.display());
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
        const FLAG_PROC: libc::c_ulong = 
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV;
        const FLAG_SYS: libc::c_ulong =
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV |
            libc::MS_RDONLY;
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
            mount(self.path.as_os_str(), self.path.as_os_str(),
                 None, libc::MS_BIND)?;
            let proc = OsString::from("proc");
            // mount(&proc, self.path.join("proc").as_os_str(),
            //     Some(&proc), FLAG_PROC)?;
            // mount(&OsString::from("sys"),
            //     self.path.join("sys").as_os_str(),
            //     Some(&OsString::from("sysfs")), FLAG_SYS)?;
            
            
            Ok(())
        })
        // sudo mount udev "${root}"/dev -t devtmpfs -o mode=0755,nosuid
        // sudo mount devpts "${root}"/dev/pts -t devpts -o mode=0620,gid=5,nosuid,noexec
        // sudo mount shm "${root}"/dev/shm -t tmpfs -o mode=1777,nosuid,nodev
        // sudo mount run "${root}"/run -t tmpfs -o nosuid,nodev,mode=0755
        // sudo mount tmp "${root}"/tmp -t tmpfs -o mode=1777,strictatime,nodev,nosuid
    }

    fn setup(&self) -> Result<(), ()> {
        assert!(self.overlay == false);
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
        std::thread::sleep(std::time::Duration::from_secs(100));
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