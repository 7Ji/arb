use std::{
        env::set_current_dir, fs::{
            create_dir, hard_link, metadata, read_dir, remove_dir, remove_dir_all, remove_file, rename, set_permissions, symlink_metadata, DirEntry, File, Metadata, ReadDir
        }, io::Write, os::unix::fs::{chown, symlink, PermissionsExt}, path::Path
    };

use nix::libc::mode_t;

use crate::{error::{
        Error,
        Result,
    }, io::reader_to_writer};

// Wrapper to check calls
pub(crate) fn remove_file_checked<P: AsRef<Path>>(path: P) -> Result<()> {
    remove_file(&path).map_err(|e|{
        log::error!("Failed to remove file '{}': {}", 
            path.as_ref().display(), e);
        e.into()
    })
}

pub(crate) fn file_create_checked<P: AsRef<Path>>(path: P) -> Result<File> {
    File::create(&path).map_err(|e| {
        log::error!("Failed to create file at '{}': {}", 
                path.as_ref().display(), e);
        e.into()
    })
}

pub(crate) fn file_create_new_checked<P: AsRef<Path>>(path: P) -> Result<File> {
    File::create_new(&path).map_err(|e| {
        log::error!("Failed to create new file at '{}': {}",
                path.as_ref().display(), e);
        e.into()
    })
}

pub(crate) fn file_open_checked<P: AsRef<Path>>(path: P) -> Result<File> {
    File::open(&path).map_err(|e| {
        log::error!("Failed to open file at '{}': {}", 
            path.as_ref().display(), e);
        e.into()
    })
}

pub(crate) fn remove_dir_checked<P: AsRef<Path>>(path: P) -> Result<()> {
    remove_dir(&path).map_err(|e| {
        log::error!("Failed to remove dir '{}' recursively: {}",
                    path.as_ref().display(), e);
        e.into()
    })
}

fn read_dir_checked<P: AsRef<Path>>(path: P) -> Result<ReadDir> {
    read_dir(&path).map_err(|e| {
        // Return failure here, but outer logic would still try to delete,
        // unlike `remove_dir_all()`
        log::error!("Failed to read dir '{}': {}", path.as_ref().display(), e);
        e.into()
    })
}

fn dir_entry_checked(entry: std::io::Result<DirEntry>) -> Result<DirEntry> {
    entry.map_err(|e|{
        log::error!("Failed to read entry from dir: {}", e);
        e.into()
    })
}

fn dir_entry_metadata_checked(entry: &DirEntry) -> Result<Metadata> {
    entry.metadata().map_err(|e|{
        log::error!("Failed to read entry metadata: {}", e);
        e.into()
    })
}

fn metadata_checked<P: AsRef<Path>>(path: P) -> Result<Metadata> {
    metadata(&path).map_err(|e| {
        log::error!("Failed to read metadata of '{}': {}", 
            path.as_ref().display(), e);
        e.into()
    })
}

fn symlink_metadata_checked<P: AsRef<Path>>(path: P) -> Result<Metadata> {
    symlink_metadata(&path).map_err(|e| {
        log::error!("Failed to read symlink metadata of '{}': {}", 
            path.as_ref().display(), e);
        e.into()
    })
}

fn create_dir_checked<P: AsRef<Path>>(path: P) -> Result<()> {
    create_dir(&path).map_err(|e|{
        log::error!("Failed to create dir at '{}': {}", 
            path.as_ref().display(), e);
        e.into()
    })
}

pub(crate) fn set_current_dir_checked<P: AsRef<Path>>(path: P) -> Result<()> {
    set_current_dir(&path).map_err(|e|{
        log::error!("Failed to set current dir to '{}': {}", 
            path.as_ref().display(), e);
        e.into()
    })
}

fn path_try_exists_checked<P: AsRef<Path>>(path: P) -> Result<bool> {
    path.as_ref().try_exists().map_err(|e|{
        log::error!("Failed to check existence of '{}': {}",
            path.as_ref().display(), e);
        e.into()
    })
}

// Our own implementations
pub(crate) fn remove_file_allow_non_existing<P: AsRef<Path>>(path: P) 
    -> Result<()> 
{
    if let Err(e) = remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::error!("Failed to remove file/symlink '{}': {}", 
                        path.as_ref().display(), e);
            return Err(e.into())
        }
    }
    Ok(())
}

fn remove_any_with_metadata<P: AsRef<Path>>(path: P, metadata: &Metadata) 
    -> Result<()> 
{
    if metadata.is_dir() {
        let er =
            remove_dir_recursively(&path);
        if let Err(e) = remove_dir_checked(&path) {
            if let Err(e) = er {
                log::error!("Failure within this dir: {}", e)
            }
            return Err(e.into())
        }
    } else {
        remove_file_allow_non_existing(&path)?
    }   
    Ok(())
}

pub(crate) fn remove_any<P: AsRef<Path>>(path: P) -> Result<()> {
    let metadata = match symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) => if e.kind() == std::io::ErrorKind::NotFound {
            return Ok(())
        } else {
            log::error!("Failed to read symlink metadata of '{}' to get file \
                type: {}", path.as_ref().display(), e);
            return Err(e.into())
        },
    };
    remove_any_with_metadata(path, &metadata)
}

/// Rmove a dir recursively, similar logic as `remove_dir_all()`, but does not 
/// fail on subdir without read permission like `build/[PKGBUILD]/pkg` before
/// being populated.
pub(crate) fn remove_dir_recursively<P: AsRef<Path>>(dir: P)
    -> Result<()>
{
    for entry in read_dir_checked(&dir)? {
        let entry = dir_entry_checked(entry)?;
        remove_any_with_metadata(&entry.path(), 
            &dir_entry_metadata_checked(&entry)?)?
    }
    Ok(())
}

/// Almost like `remove_dir_all()`, but try `remove_dir_all()` first and then
/// use our own implementation `remove_dir_recursively()` if that fails, 
/// primarily to cover the case where subdir `build/[PKGBUILD]/pkg` is without 
/// read permission before being populated.
pub(crate) fn remove_dir_all_try_best<P: AsRef<Path>>(dir: P) -> Result<()>
{
    log::debug!("Removing dir '{}' recursively...", dir.as_ref().display());
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

pub(crate) fn _create_dir_all_under_owned_by<P, Q>(
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
                create_dir_checked(&path),
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
    create_dirs_under_allow_existing(
        ["pacman.sync"], "build")?;
    remove_dirs_allow_non_existing(["pkgs/updated", "pkgs/latest"])?;
    create_dirs_under_allow_existing(
        ["updated", "latest", "cache"], "pkgs")?;
    create_dirs_under_allow_existing([
        "file-ck", "file-md5", "file-sha1", "file-sha224", "file-sha256",
        "file-sha384", "file-sha512", "file-b2", "git", "PKGBUILD", "pkg"], 
        "sources")
}

pub(crate) fn prepare_layout() -> Result<()> {
    create_layout()
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

pub(crate) fn set_permissions_mode<P: AsRef<Path>>(path: P, mode: mode_t) 
-> Result<()> 
{
    let path = path.as_ref();
    if let Err(e) = set_permissions(
        path, PermissionsExt::from_mode(mode))
    {
        log::error!("Failed to set permissions for '{}' to {:o}: {}", 
            path.display(), mode, e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn touch<P: AsRef<Path>>(path: P) -> Result<()> {
    if let Err(e) = std::fs::OpenOptions::new()
                            .create(true).write(true).open(&path) 
    {
        log::error!("Failed to touch file '{}': {}", 
                        path.as_ref().display(), e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn action_rm_rf<I, P>(paths: I) -> Result<()> 
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>
{
    crate::rootless::try_unshare_user_and_wait()?;
    for path in paths {
        remove_any(path)?
    }
    Ok(())
}


pub(crate) fn file_create_with_content<P, B>(path: P, content: B) -> Result<()>
where
    P: AsRef<Path>, 
    B: AsRef<[u8]>
{
    let path = path.as_ref();
    let content = content.as_ref();
    let mut file = file_create_checked(path)?;
    if let Err(e) = file.write_all(content) {
        log::error!("Failed to write {} bytes of content into '{}': {}",
            content.len(), path.display(), e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn clone_file<P1, P2>(source: P1, target: P2)
    -> Result<()>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>
{
    let source = source.as_ref();
    let target = target.as_ref();
    if path_try_exists_checked(target)? {
        remove_any(target)?
    }
    if let Err(e) = hard_link(&source, &target) {
        log::error!("Failed to link {} to {}: {}, trying heavy copy",
                    target.display(), source.display(), e)
    } else {
        return Ok(())
    }
    let mut target_file =  file_create_checked(target)?;
    let mut source_file = file_open_checked(source)?;
    match reader_to_writer(
        &mut source_file, &mut target_file
    ) {
        Ok(_) => {
            log::info!("Cloned '{}' to '{}'", 
                source.display(), target.display());
            Ok(())
        },
        Err(e) => {
            log::error!("Failed to hard copy '{}' to '{}': {}", 
                source.display(), target.display(), e);
            Err(e)
        }
    }
}

pub(crate) fn rename_checked<P1, P2>(source: P1, target: P2) -> Result<()> 
where
    P1: AsRef<Path>,
    P2: AsRef<Path>
{
    let source = source.as_ref();
    let target = target.as_ref();
    if let Err(e) = rename(source, target) {
        log::error!("Failed to rename '{}' to '{}': {}", 
            source.display(), target.display(), e);
    }
    Ok(())
}


pub(crate) fn move_file<P1, P2>(source: P1, target: P2) -> Result<()> 
where
    P1: AsRef<Path>,
    P2: AsRef<Path>
{
    let source = source.as_ref();
    let target = target.as_ref();
    if rename_checked(source, target).is_ok() {
        return Ok(())
    }
    log::warn!("Failed to rename to lightweight move, \
                trying clone then delete old");
    clone_file(source, target)?;
    remove_file_checked(source)
}