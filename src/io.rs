use std::io::{Read, Write};
use is_terminal;
use crate::{filesystem::file_create_checked, Result};

pub(crate) fn flush_stdout() -> Result<()> {
    if let Err(e) = std::io::stdout().flush() {
        log::error!("Failed to flush stdout: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn write_all_to_stdout<C: AsRef<[u8]>>(content: C) -> Result<()> {
    if let Err(e) = std::io::stdout().write_all(content.as_ref()) {
        log::error!("Failed to write all remote log to stdout: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn is_stdout_terminal() -> bool {
    is_terminal::is_terminal(&std::io::stdout())
}

pub(crate) fn write_all_to_file_or_stdout<B: AsRef<[u8]>>(buffer: B, out: &str) 
    -> Result<()> 
{
    if let Err(e) = 
        if out == "-" {
            std::io::stdout().write_all(buffer.as_ref())
        } else {
            file_create_checked(out)?.write_all(buffer.as_ref())
        }
    {
        log::error!("Failed to write to file or stdout '{}': {}", out, e);
        Err(e.into())
    } else {
        Ok(())
    }
}

const BUFFER_SIZE: usize = 0x100000;

pub(crate) fn reader_to_writer<R: Read, W: Write>(mut reader: R, mut writer: W) 
    -> Result<()>
{
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let size_chunk = match
            reader.read(&mut buffer) {
                Ok(size) => size,
                Err(e) => {
                    log::error!("Failed to read from reader: {}", e);
                    return Err(e.into())
                },
            };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        if let Err(e) = writer.write_all(chunk) {
            log::error!(
                "Failed to write {} bytes into writer : {}",
                size_chunk, e);
            return Err(e.into());
        }
    }
    Ok(())
}