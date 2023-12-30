use std::{
        fs::{
            create_dir,
            File,
            read_dir,
            remove_dir,
            remove_dir_all,
            remove_file,
        },
        os::unix::fs::{chown, symlink},
        path::Path,
    };

use crate::error::{
        Error,
        Result,
    };

/// Rmove a dir recursively, similar logic as `remove_dir_all()`, but does not 
/// fail on subdir without read permission like `build/[PKGBUILD]/pkg` before
/// being populated.
pub(crate) fn remove_dir_recursively<P: AsRef<Path>>(dir: P)
    -> Result<()>
{
    let readdir = match read_dir(&dir) {
        Ok(readdir) => readdir,
        Err(e) => {
            // Return failure here, but outer logic would still try to delete,
            // unlike `remove_dir_all()`
            log::error!("Failed to read dir '{}': {}", 
                        dir.as_ref().display(), e);
            return Err(e.into())
        },
    };
    for entry in readdir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("Failed to read entry from dir '{}': {}",
                            dir.as_ref().display(), e);
                return Err(e.into())
            },
        };
        let path = entry.path();
        // Only recursive on real dir
        if !path.is_symlink() && path.is_dir() {
            let er =
                remove_dir_recursively(&path);
            if let Err(e) =  remove_dir(&path) {
                log::error!("Failed to remove subdir '{}' recursively: {}",
                            path.display(), e);
                if let Err(e) = er {
                    log::error!("Failure within subdir: {}", e)
                }
                return Err(e.into())
            }
        } else if let Err(e) = remove_file(&path) {
            log::error!("Failed to remove entry file/symlink '{}': {}", 
                        path.display(), e);
            return Err(e.into())
        }
    }
    Ok(())
}

/// Almost like `remove_dir_all()`, but try `remove_dir_all()` first and then
/// use our own implementation `remove_dir_recursively()` if that fails, 
/// primarily to cover the case where subdir `build/[PKGBUILD]/pkg` is without 
/// read permission before being populated.
pub(crate) fn remove_dir_all_try_best<P: AsRef<Path>>(dir: P) -> Result<()>
{
    log::info!("Removing dir '{}' recursively...", dir.as_ref().display());
    match remove_dir_all(&dir) {
        Ok(_) => return Ok(()),
        Err(e) => {
            log::warn!("Failed to remove dir '{}' recursively naively: {}",
                dir.as_ref().display(), e);
        },
    }
    if let Err(e) = remove_dir_recursively(&dir) {
        log::error!("Failed to remove dir '{}' entries recursively: {}",
            dir.as_ref().display(), e);
        return Err(e.into())
    }
    if let Err(e) = remove_dir(&dir) {
        log::error!("Failed to remove dir '{}' itself: {}",
            dir.as_ref().display(), e);
        return Err(e.into())
    }
    log::info!("Removed dir '{}' recursively", dir.as_ref().display());
    Ok(())
}

pub(crate) fn create_dir_all_under_owned_by<P, Q>(
    path: P, parent: Q, uid: u32, gid: u32
) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>
{
    let mut path_buffer = parent.as_ref().to_owned();
    for component in path.as_ref().components() {
        path_buffer.push(component);
        if ! path_buffer.exists() {
            if let Err(e) = create_dir(&path_buffer) {
                log::error!("Failed to create dir '{}': {}",
                    path_buffer.display(), e);
                return Err(e.into())
            }
        }
        if let Err(e) = chown(
            &path_buffer, Some(uid), Some(gid)) 
        {
            log::error!("Failed to chown '{}' to {}:{}: {}",
                path_buffer.display(), uid, gid, e);
            return Err(e.into())
        }
    }
    Ok(())
}

pub(crate) fn create_dir_allow_existing<P: AsRef<Path>>(path: P) -> Result<()> {
    let metadata = match path.as_ref().symlink_metadata() {
        Ok(metadata) => metadata,
        Err(e) => return match e.kind() {
            std::io::ErrorKind::NotFound => 
                if let Err(e) = create_dir(&path) {
                    log::error!("Failed to create dir at '{}': {}", 
                        path.as_ref().display(), e);
                    Err(e.into())
                } else {
                    Ok(())
                },
            _ => {
                log::error!("Failed to get metadata of '{}': {}", 
                    path.as_ref().display(), e);
                Err(e.into())
            },
        },
    };
    if metadata.is_dir() {
        Ok(())
    } else {
        log::error!("Cannot create dir at '{}' which already exists and \
            is not a dir", path.as_ref().display());
        Err(Error::FilesystemConflict)
    }
}

/// Attempt to run `create_dir_allow_existing()` for all entry in iterator, 
/// try them all, only return the last error (if any)
pub(crate) fn create_dirs_allow_existing<I, P>(dirs: I) -> Result<()> 
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>
{
    let mut r = Ok(());
    for dir in dirs {
        if let Err(e) = create_dir_allow_existing(dir) {
            r = Err(e)
        }
    }
    r
}

/// Attempt to run `create_dir_under_allow_existing()` for all entry in 
/// iterator, try them all, only return the last error (if any)
pub(crate) fn create_dirs_under_allow_existing<I, P>(dirs: I, under: P) -> Result<()> 
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>
{
    let mut r = Ok(());
    let mut path = under.as_ref().to_owned();
    for dir in dirs {
        path.push(dir);
        if let Err(e) = create_dir_allow_existing(&path) {
            r = Err(e)
        }
        path.pop();
    }
    r
}

pub(crate) fn remove_dir_allow_non_existing<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        if ! path.is_dir() {
            log::error!("Cannot remove dir at '{}' which already exists but \
                is not a dir", path.display());
            return Err(Error::FilesystemConflict)
        }
        if let Err(e) = remove_dir_all_try_best(&path) {
            log::error!("Failed to remove dir at '{}': {}", path.display(), e);
            return Err(e.into())
        }
    }
    Ok(())
}

pub(crate) fn remove_dirs_allow_non_existing<I, P>(dirs: I) -> Result<()> 
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>
{
    let mut r = Ok(());
    for dir in dirs {
        if let Err(e) = remove_dir_allow_non_existing(dir) {
            r = Err(e)
        }
    }
    r
}


pub(crate) fn create_layout() -> Result<()> {
    create_dirs_allow_existing(["build", "logs", "pkgs", "sources"])?;
    remove_dirs_allow_non_existing(["pkgs/updated", "pkgs/latest"])?;
    create_dirs_under_allow_existing(["updated", "latest"], "pkgs")?;
    create_dirs_under_allow_existing([
        "file-ck", "file-md5", "file-sha1", "file-sha224", "file-sha256",
        "file-sha384", "file-sha512", "file-b2", "git", "PKGBUILD"], 
        "sources")
}

pub(crate) fn symlink_force<P, Q>(original: P, link: Q) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    if let Err(e) = symlink(&original, &link) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            log::error!("Failed to symlink '{}' to '{}': {}", 
                original.as_ref().display(), link.as_ref().display(), e);
            return Err(e.into())
        }
        log::warn!("Symlink target '{}' exists, trying to remove it",
                            link.as_ref().display());
        let metadata = match link.as_ref().symlink_metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                log::error!("Failed to get metadata of '{}': {}",
                    original.as_ref().display(), e);
                return Err(e.into())
            },
        };
        if metadata.is_dir() {
            log::info!("Removing symlink target which is a dir...");
            remove_dir_all_try_best(&original)?;
        // } else if metadata.is_file() || metadata.is_symlink() {
        } else {
            log::info!("Removing symlink target which is not a dir...");
            remove_file(&link).map_err(|e|Error::IoError(e))?
        }
        if let Err(e) = symlink(&original, &link) {
            log::error!("Failed to force symlink '{}' to '{}': {}",
                original.as_ref().display(), link.as_ref().display(), e);
            Err(e.into())
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}