use std::{fs::File, io::{BufRead, BufReader,  BufWriter, Read, Write}, ops::DerefMut, path::Path, sync::{Arc, Mutex, MutexGuard}, thread::JoinHandle, time::Instant};
use is_terminal;
use crate::{error::Error, filesystem::file_create_checked, Result};

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

pub(crate) fn prefixed_reader_to_shared_writer<R, W, S>(reader: R, writer: Arc<Mutex<BufWriter<W>>>, prefix: S, time_start: Instant) -> Result<()>
where
    R: Read,
    W: Write,
    S: AsRef<str>,
{
    let prefix = prefix.as_ref();
    for line in BufReader::new(reader).lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                log::error!("Failed to read line: {}", e);
                return Err(e.into())
            },
        };
        let mut writer = match writer.lock() {
            Ok(writer) => writer,
            Err(_) => {
                log::error!("Failed to get writer");
                return Err(Error::ThreadFailure(None))
            },
        };
        let elapsed = (Instant::now() - time_start).as_secs_f64();
        if let Err(e) = writer.get_mut().write_fmt(format_args!("[{:12.6}/{}] {}\n", elapsed, prefix, line)) {
            log::error!("Failed to write line: {}", e);
            return Err(e.into())
        }
    }
    Ok(())
}

pub(crate) fn reader_to_buffer<R: Read>(mut reader: R) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    match reader.read_to_end(&mut buffer) {
        Ok(size) => {
            log::debug!("Read {} bytes from buffer", size);
            Ok(buffer)
        },
        Err(e) => {
            log::error!("Failed to read from reader into buffer");
            Err(e.into())
        },
    }
}

type RawBufferedFile = BufWriter<File>;
type RawMTSharedBufferedFile = Arc<Mutex<RawBufferedFile>>;

pub(crate) struct MTSharedBufferedFile {
    file: RawMTSharedBufferedFile
}

impl MTSharedBufferedFile {
    pub(crate) fn new(file: File) -> Self {
        Self {
            file: Arc::new(Mutex::new(BufWriter::new(file)))
        }
    }

    pub(crate) fn clone_raw(&self) -> RawMTSharedBufferedFile {
        self.file.clone()
    }

    fn try_lock(&self) -> Result<MutexGuard<RawBufferedFile>> {
        match self.file.lock() {
            Ok(file) => Ok(file),
            Err(_) => {
                log::error!("Failed to lock log file");
                Err(Error::ThreadFailure(None))
            },
        }
    }

    pub(crate) fn write_fmt(&self, fmt: std::fmt::Arguments<'_>) -> Result<()> {
        let mut writer = self.try_lock()?;
        if let Err(e) = writer.write_fmt(fmt) {
            log::error!("Failed to write format args to log file");
            Err(e.into())
        } else {
            Ok(())
        }
    }

    pub(crate) fn write_fmt_nobuf(&self, fmt: std::fmt::Arguments<'_>) -> Result<()> {
        let mut writer = self.try_lock()?;
        if let Err(e) = writer.get_mut().write_fmt(fmt) {
            log::error!("Failed to write format args to log file");
            Err(e.into())
        } else {
            Ok(())
        }
    }
}