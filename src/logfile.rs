use std::{ffi::OsString, fs::File, path::PathBuf, time::Instant};
use rand::{distributions::Alphanumeric, Rng};

use crate::{filesystem::{file_create_checked, file_create_new_checked}, io::MTSharedBufferedFile, Error, Result};

const DATE_TIME_FORMAT: &[time::format_description::FormatItem<'_>] = 
    time::macros::format_description!(
        "[year][month][day]_[hour][minute][second]");

fn time_now_utc_formatted() -> String {
    match time::OffsetDateTime::now_utc().format(DATE_TIME_FORMAT) 
    {
        Ok(time_formatted) => time_formatted,
        Err(e) => {
            log::warn!("Failed to format time: {}", e);
            "19700101_000000".into()
        },
    }
}

pub(crate) struct LogFileBuilder {
    stem: String,
}

impl LogFileBuilder {
    pub(crate) fn new(log_file_type: &str, suffix: &str) -> Self {
        let mut stem: String = log_file_type.into();
        if ! suffix.is_empty() {
            stem.push('-');
            for part in 
                suffix.trim().split_whitespace()
                    .filter(|s|! s.is_empty()) 
            {
                stem.push_str(part)
            }
        }
        Self { stem }
    }

    pub(crate) fn try_create(&self) -> Result<LogFile> {
        let logs = PathBuf::from("logs");
        for i in 0..100 {
            let mut name = time_now_utc_formatted();
            name.push('-');
            name.push_str(&self.stem);
            if i > 0 {
                name.push('-');
                rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(7)
                    .for_each(|c|name.push(c.into()))
            }
            name.push_str(".log");
            let path = logs.join(name);
            let file = file_create_new_checked(&path)?;
            return Ok( LogFile { path, file } )
        }
        log::error!("Failed to create log file after 100 tries");
        Err(Error::FilesystemConflict)
    }
}

pub(crate) struct LogFile {
    pub(crate) path: PathBuf,
    pub(crate) file: File,
}

impl Into<PathBuf> for LogFile {
    fn into(self) -> PathBuf {
        self.path
    }
}

impl Into<OsString> for LogFile {
    fn into(self) -> OsString {
        self.path.into()
    }
}

impl TryFrom<OsString> for LogFile {
    type Error = Error;

    fn try_from(value: OsString) -> Result<Self> {
        let path = PathBuf::from(value);
        let file = file_create_checked(&path)?;
        Ok(Self { path, file })
    }
}

impl LogFile {
    pub(crate) fn into_pathbuf(self) -> PathBuf {
        self.into()
    }

    pub(crate) fn into_os_string(self) -> OsString {
        self.into()
    }

    pub(crate) fn try_split(self) -> Result<(File, File)> {
        let file_dup = self.file.try_clone()?;
        Ok((self.file, file_dup))
    }

    pub(crate) fn try_new(log_file_type: &str, suffix: &str) 
        -> Result<Self> 
    {
        LogFileBuilder::new(log_file_type, suffix).try_create()
    }

    pub(crate) fn try_open<O: Into<OsString>>(path: O) -> Result<Self> {
        Self::try_from(path.into())
    }
}

impl MTSharedBufferedFile {
    pub(crate) fn write_start(&self) -> Result<()> {
        self.write_fmt_nobuf(
            format_args!("[    0.000000/---] --- begin at {}\n", 
            time::OffsetDateTime::now_utc()))
    }

    pub(crate) fn write_end(&self, time_start: Instant) -> Result<()> {
        let elapsed = (Instant::now() - time_start).as_secs_f64();
        self.write_fmt(
            format_args!("[{:12.6}/---] --- end of log file ---\n", elapsed))
    }
}