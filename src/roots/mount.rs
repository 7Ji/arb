use std::{path::{PathBuf, Path}, ffi::CString, os::unix::prelude::OsStrExt, fs::remove_dir_all};

use crate::identity::{IdentityActual, Identity};



#[derive(Clone)]
pub(super) struct MountedFolder (pub(super) PathBuf);


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

pub(super) fn mount(
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
    pub(super) fn umount_recursive(&self) -> Result<&Self, ()> {
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
    pub(super) fn remove(&self) -> Result<&Self, ()> {
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
    pub(super) fn remove_all() -> Result<(), ()> {
        Self(PathBuf::from("roots")).remove().and(Ok(()))
    }
}

impl Drop for MountedFolder {
    fn drop(&mut self) {
        if IdentityActual::as_root(||{
            self.remove().and(Ok(()))
        }).is_err() {
            eprintln!("Failed to drop mounted folder '{}'", self.0.display());
        }
    }
}
