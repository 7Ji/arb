use std::{ffi::OsString, fmt::Display, fs::File, path::{Path, PathBuf}, process::Child};
use rand::{distributions::Alphanumeric, Rng};

use crate::{Error, Result};

// pub(crate) enum LogFileType {
//     Pacman,
//     Localedef,
//     Extract,
//     Build,
// }

// impl Into<&str> for &LogFileType {
//     fn into(self) -> &'static str {
//         match self {
//             LogFileType::Pacman => "pacman",
//             LogFileType::Localedef => "localedef",
//             LogFileType::Extract => "extract",
//             LogFileType::Build => "build",
//         }
//     }
// }

// impl LogFileType {
//     fn to_str(&self) -> &'static str {
//         self.into()
//     }
// }

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
        const DATE_TIME_FORMAT: &[time::format_description::FormatItem<'_>] = 
            time::macros::format_description!(
                "[year][month][day]_[hour][minute][second]");
        let logs = PathBuf::from("logs");
        for i in 0..100 {
            let mut name = match 
                time::OffsetDateTime::now_utc().format(DATE_TIME_FORMAT) 
            {
                Ok(time_formatted) => 
                    format!("{}_{}", time_formatted, &self.stem),
                Err(e) => {
                    log::warn!("Failed to format time: {}", e);
                    format!("19700101_000000_{}", &self.stem)
                },
            };
            if i > 0 {
                name.push('-');
                rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(7)
                    .for_each(|c|name.push(c.into()))
            }
            name.push_str(".log");
            let path = logs.join(name);
            match File::create_new(&path) {
                Ok(file) => return Ok( LogFile { path, file } ),
                Err(e) => {
                    log::error!("Failed to create log file at '{}': {}",
                        path.display(), e);
                },
            }
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
        let file = File::create(&path)?;
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
}