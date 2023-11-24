use std::{
        fs::{
            create_dir,
            File,
            read_dir,
            remove_dir,
            remove_dir_all,
            remove_file,
        },
        io::{
            Read,
            stdout,
            Write
        },
        os::unix::fs::{chown, symlink},
        path::Path,
    };

use crate::error::{
        Error,
        Result,
    };

// build/*/pkg being 0111 would cause remove_dir_all() to fail, in this case
// we use our own implementation
pub(crate) fn remove_dir_recursively<P: AsRef<Path>>(dir: P)
    -> Result<()>
{
    for entry in 
        read_dir(&dir).map_err(|e|Error::IoError(e))? 
    {
        let entry = entry.map_err(|e|Error::IoError(e))?;
        let path = entry.path();
        if !path.is_symlink() && path.is_dir() {
            let er =
                remove_dir_recursively(&path);
            match remove_dir(&path) {
                Ok(_) => (),
                Err(e) => {
                    log::error!(
                        "Failed to remove subdir '{}' recursively: {}",
                        path.display(), e);
                    if let Err(e) = er {
                        log::error!("Subdir failure: {}", e)
                    }
                    return Err(Error::IoError(e));
                },
            }
        } else {
            remove_file(&path).map_err(|e|Error::IoError(e))?
        }
    }
    let a = nix::errno::Errno::E2BIG;
    println!("{}", a);
    Ok(())
}


pub(crate) fn remove_dir_all_try_best<P: AsRef<Path>>(dir: P)
    -> Result<()>
{
    log::info!("Removing dir '{}' recursively...", dir.as_ref().display());
    match remove_dir_all(&dir) {
        Ok(_) => return Ok(()),
        Err(e) => {
            log::error!("Failed to remove dir '{}' recursively naively: {}",
                dir.as_ref().display(), e);
        },
    }
    remove_dir_recursively(&dir).or_else(|e|{
        log::error!("Failed to remove dir '{}' recursively: {}",
            dir.as_ref().display(), e);
        Err(e)
    })?;
    remove_dir(&dir).or_else(|e|{
        log::error!("Failed to remove dir '{}' itself: {}",
            dir.as_ref().display(), e);
        Err(Error::IoError(e))
    })?;
    log::info!("Removed dir '{}' recursively", dir.as_ref().display());
    Ok(())
}

pub(crate) fn _file_to_stdout<P: AsRef<Path>>(file: P) -> Result<()> {
    let file_p = file.as_ref();
    let mut file = match File::open(&file) {
        Ok(file) => file,
        Err(e) => {
            log::error!("Failed to open '{}': {}", file_p.display(), e);
            return Err(Error::IoError(e))
        },
    };
    let mut buffer = vec![0; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(size) => {
                if size == 0 {
                    return Ok(())
                }
                if let Err(e) = stdout().write_all(&buffer[0..size])
                {
                    log::error!("Failed to write log content to stdout: {}", e);
                    return Err(Error::IoError(e))
                }
            },
            Err(e) => {
                log::error!("Failed to read from '{}': {}", file_p.display(), e);
                return Err(Error::IoError(e))
            },
        }
    }
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
            create_dir(&path_buffer).map_err(|e| {
                log::error!("Failed to create dir '{}': {}",
                    path_buffer.display(), e);
                Error::IoError(e)
            })?;
        }
        chown(&path_buffer, Some(uid), Some(gid)).map_err(
            |e| {
                log::error!("Failed to chown '{}' to {}:{}: {}",
                    path_buffer.display(), uid, gid, e);
                Error::IoError(e)
            })?;
    }
    Ok(())
}

pub(crate) fn create_dir_allow_existing<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    if path.exists() {
        if ! path.is_dir() {
            log::error!("Cannot create dir at '{}' which already exists and \
                is not a dir", path.display());
            return Err(Error::FilesystemConflict)
        }
    } else {
        if let Err(e) = create_dir(&path) {
            log::error!("Failed to create dir at '{}': {}", path.display(), e);
            return Err(e.into())
        }
    }
    Ok(())
}

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
    match symlink(&original, &link) {
        Ok(()) => Ok(()),
        Err(e) => if e.kind() == std::io::ErrorKind::AlreadyExists {
            log::warn!("Symlink target '{}' exists, trying to remove it",
                                link.as_ref().display());
            let metadata = match original.as_ref().metadata() {
                Ok(metadata) => metadata,
                Err(e) => {
                    log::error!("Failed to get metadata of '{}': {}",
                        original.as_ref().display(), e);
                    return Err(e.into())
                },
            };
            if metadata.is_dir() {
                remove_dir_all_try_best(&original)?;
            } else if metadata.is_file() {
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
            log::error!("Failed to symlink '{}' to '{}': {}", 
                original.as_ref().display(), link.as_ref().display(), e);
            Err(e.into())
        },
    }

}