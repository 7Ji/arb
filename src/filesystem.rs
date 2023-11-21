use std::{
        fs::{
            create_dir,
            create_dir_all,
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
        os::unix::fs::chown,
        path::{
            Path,
            PathBuf
        },
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

pub(crate) fn file_to_stdout<P: AsRef<Path>>(file: P) -> Result<()> {
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

pub(crate) fn prepare_updated_latest_dirs() -> Result<()> {
    let mut bad = false;
    let dir = PathBuf::from("pkgs");
    let mut r = Ok(());
    for subdir in ["updated", "latest"] {
        let dir = dir.join(subdir);
        if dir.exists() {
            if let Err(e) = remove_dir_all_try_best(&dir) {
                r = Err(e)
            }
        }
        if let Err(e) = create_dir_all(&dir) {
            log::error!("Failed to create dir '{}': {}", dir.display(), e);
            r = Err(Error::IoError(e))
        }
    }
    r
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
        create_dir(&path).map_err(|e|{
            log::error!("Failed to create dir at '{}': {}", path.display(), e);
            Error::IoError(e)
        })?
    }
    Ok(())
}

pub(crate) fn prepare_pkgdir() -> Result<()> {
    let mut path = PathBuf::from("pkgs");
    for suffix in ["updated", "latest"] {
        path.push(suffix);
        if ! path.is_dir() {
            log::error!("Existing '{}' is not folder", path.display());
            return Err(Error::FilesystemConflict)
        }
        if let Err(e) = remove_dir_all(&path) {
            log::error!("Failed to remove dir '{}': {}", path.display(), e);
            return Err(e.into())
        }
        if let Err(e) = create_dir_all((&path)) {
            log::error!("Failed to create dir '{}': {}", path.display(), e);
            return Err(e.into())
        }
        path.pop();
    }
    Ok(())
}
