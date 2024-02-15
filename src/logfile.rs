// Log to be used in 

use std::{
        fmt::Display,
        fs::File,
        path::PathBuf, 
        process::Command,
    };

use time;

use crate::error::{
        Error,
        Result,
    };

pub(crate) enum LogType {
    Build,
    Extract,
    Pacman,
}

impl Display for LogType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Build => "build",
            Self::Extract => "extract",
            Self::Pacman => "pacman"
        })
    }
}

pub(crate) struct LogFile {
    pub(crate) path: PathBuf,
    pub(crate) file: File
}

impl LogFile {
    pub(crate) fn new<S: AsRef<str>>(log_type: LogType, id: S) -> Result<Self> {
        const DATE_TIME_FORMAT: &[time::format_description::FormatItem<'_>] = 
            time::macros::format_description!(
                "[year][month][day]_[hour][minute][second]");
        let time_formatted = match 
            time::OffsetDateTime::now_utc().format(DATE_TIME_FORMAT) 
        {
            Ok(time_formatted) => time_formatted,
            Err(e) => {
                log::error!("Failed to format time: {}", e);
                return Err(Into::<time::Error>::into(e).into())
            },
        };
        let path = PathBuf::from(format!("logs/{}_{}_{}.log", 
            time_formatted, log_type, id.as_ref()));
        let file = File::create(&path).map_err(Error::from)?;
        log::info!("Log for {} '{}' is stored at '{}'", log_type, id.as_ref(), 
                    path.display());
        Ok(Self {
            path,
            file,
        })
    }

    pub(crate) fn set_command(self, command: &mut Command) 
        -> Result<&mut Command> 
    {
        let dup_file = match self.file.try_clone() {
            Ok(dup_file) => dup_file,
            Err(e) => {
                log::error!("Failed to dup log file handle: {}", e);
                return Err(e.into())
            },
        };
        Ok(command.stderr(dup_file).stdout(self.file))
    }
}