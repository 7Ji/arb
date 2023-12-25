use std::{
        fs::{
            File,
            hard_link,
            remove_file,
        },
        io::{
            Read,
            Write,
        },
        path::{
            Path,
            PathBuf
        },
    };

use crate::error::{
        Error,
        Result
    };

pub(crate) fn clone_file(source: &Path, target: &Path)
    -> Result<()>
{
    if target.exists() {
        if let Err(e) = remove_file(&target) {
            log::error!("Failed to remove file {}: {}",
                &target.display(), e);
            return Err(Error::IoError(e))
        }
    }
    match hard_link(&source, &target) {
        Ok(_) => return Ok(()),
        Err(e) =>
            log::error!("Failed to link {} to {}: {}, trying heavy copy",
                        target.display(), source.display(), e),
    }
    let mut target_file = match File::create(&target) {
        Ok(target_file) => target_file,
        Err(e) => {
            log::error!("Failed to open {} as write-only: {}",
                        target.display(), e);
            return Err(Error::IoError(e))
        },
    };
    let mut source_file = match File::open(&source) {
        Ok(source_file) => source_file,
        Err(e) => {
            log::error!("Failed to open {} as read-only: {}",
                        source.display(), e);
            return Err(Error::IoError(e))
        },
    };
    let mut buffer = vec![0; super::BUFFER_SIZE];
    loop {
        let size_chunk = match
            source_file.read(&mut buffer) {
                Ok(size) => size,
                Err(e) => {
                    log::error!("Failed to read file: {}", e);
                    return Err(Error::IoError(e))
                },
            };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        if let Err(e) = target_file.write_all(chunk) {
            log::error!(
                "Failed to write {} bytes into file '{}': {}",
                size_chunk, target.display(), e);
            return Err(Error::IoError(e));
        }
    }
    log::info!("Cloned '{}' to '{}'", source.display(), target.display());
    Ok(())
}

pub(crate) fn file(url: &str, path: &Path) -> Result<()> {
    if ! url.starts_with("file://") {
        log::error!("URL '{}' does not start with file://", url);
        return Err(Error::InvalidConfig)
    }
    clone_file(&PathBuf::from(&url[7..]), path)
}