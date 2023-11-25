// Log to be used in 

use std::{
        fmt::Display,
        fs::File,
        path::PathBuf,
    };

use time;

use crate::error::{
        Error,
        Result,
    };

pub(crate) enum LogType {
    Build,
    Extract,
}

impl Display for LogType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Self::Build => "build",
            Self::Extract => "extract"
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
        log::info!("New log file created at '{}'", path.display());
        Ok(Self {
            path,
            file,
        })
    }
}

// pub(crate) struct TimedLogFile {
//     start: Instant,
//     childout: ChildStdout,
//     childerr: ChildStderr,
//     inner: LogFile,
// }

// impl TimedLogFile {
//     pub(crate) fn new<S: AsRef<str>>(log_type: LogType, id: S) -> Result<Self> {
//         let inner = LogFile::new(log_type, id)?;
//         Ok(Self{
//             start: Instant::now(),
//             inner,
//         })
//     }
//     fn take_child(child: &mut Child) {
        
//     }

// }