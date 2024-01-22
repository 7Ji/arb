use std::io::Write;
use is_terminal;
use crate::Result;

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