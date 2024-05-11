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

pub(crate) fn reader_to_writer<R, W>(reader: &mut R, writer: &mut W) 
    -> Result<()>
where
    R: Read, 
    W: Write
{
    if let Err(e) = std::io::copy(reader, writer) {
        log::error!("Failed to to copy from reader to writer: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}