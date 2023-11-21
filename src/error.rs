#[derive(Debug)]
pub(crate) enum Error {
    AlpmError (alpm::Error),
    BadChild {
        pid: Option<nix::unistd::Pid>,
        code: Option<i32>,
    },
    BrokenEnvironment,
    BrokenPKGBUILDs (Vec<String>),
    BuildFailure,
    Collapsed(String),
    DependencyMissing (Vec<String>),
    FilesystemConflict,
    GitError (git2::Error),
    GitObjectMissing,
    ImpossibleLogic,
    IntegrityError,
    InvalidConfig,
    // MappingFailure,
    IoError (std::io::Error),
    NixErrno (nix::errno::Errno),
    ProcError (procfs::ProcError),
    ThreadFailure (Option<Box<dyn std::any::Any + Send + 'static>>),
    TimeError (time::Error),
    UreqError (ureq::Error),
    UrlParseError (url::ParseError),
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

impl Default for Error {
    fn default() -> Self {
        Self::ImpossibleLogic
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AlpmError(e) => write!(f, "Alpm Error: {}", e),
            Error::BadChild { pid, code } => write!(f, "Bad child, pid {:?}, code {:?}", pid, code),
            Error::BrokenEnvironment => write!(f, "Broken Environment"),
            Error::BrokenPKGBUILDs(pkgbuilds) => write!(f, "Broken PKGBUILDs: {:?}", pkgbuilds),
            Error::BuildFailure => write!(f, "Build Failure"),
            Error::Collapsed(s) => write!(f, "Collapsed {}", s),
            Error::DependencyMissing( deps ) => write!(f, "Dependency missing: {:?}", deps),
            Error::FilesystemConflict => write!(f, "Filesystem Conflict"),
            Error::GitObjectMissing => write!(f, "Git Object Missing"),
            Error::GitError(e) => write!(f, "Git Error: {}", e),
            Error::ImpossibleLogic => write!(f, "Impossible Logic"),
            Error::IntegrityError => write!(f, "Integrity Error"),
            Error::InvalidConfig => write!(f, "Invalid Config"),
            Error::IoError(e) => write!(f, "IO Error: {}", e),
            Error::NixErrno(e) => write!(f, "Nix Errno: {}", e),
            Error::ProcError(e) => write!(f, "Proc Error: {}", e),
            Error::ThreadFailure(artifact) => write!(f, "Thread Failure, artifact: {:?}", artifact),
            Error::TimeError(e) => write!(f, "Time Error: {}", e),
            Error::UreqError(e) => write!(f, "Ureq Error: {}", e),
            Error::UrlParseError(e) => write!(f, "URL Parse Error: {}", e),
        }
    }
}

impl From<alpm::Error> for Error {
    fn from(value: alpm::Error) -> Self {
        Self::AlpmError(value)
    }
}

impl From<git2::Error> for Error {
    fn from(value: git2::Error) -> Self {
        Self::GitError(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::IoError(value)
    }
}

impl From<nix::errno::Errno> for Error {
    fn from(value: nix::errno::Errno) -> Self {
        Self::NixErrno(value)
    }
}

impl From<procfs::ProcError> for Error {
    fn from(value: procfs::ProcError) -> Self {
        Self::ProcError(value)
    }
}

impl From<ureq::Error> for Error {
    fn from(value: ureq::Error) -> Self {
        Self::UreqError(value)
    }
}

impl From<url::ParseError> for Error {
    fn from(value: url::ParseError) -> Self {
        Self::UrlParseError(value)
    }
}

impl From<time::Error> for Error {
    fn from(value: time::Error) -> Self {
        Self::TimeError(value)
    }
}

impl Into<std::io::Error> for Error {
    fn into(self) -> std::io::Error {
        match self {
            Error::IoError(e) => e,
            Error::NixErrno(e) => e.into(),
            _ => std::io::Error::new(std::io::ErrorKind::Other, "Unmappable Error")
        }
    }
}

impl Clone for Error {
    fn clone(&self) -> Self {
        match self {
            Self::AlpmError(arg0) => Self::AlpmError(arg0.clone()),
            Self::BadChild { pid, code } => Self::BadChild { pid: pid.clone(), code: code.clone() },
            Self::BrokenEnvironment => Self::BrokenEnvironment,
            Self::BrokenPKGBUILDs(arg0) => Self::BrokenPKGBUILDs(arg0.clone()),
            Self::BuildFailure => Self::BuildFailure,
            Self::Collapsed(arg0) => Self::Collapsed(arg0.clone()),
            Self::DependencyMissing(arg0) => Self::DependencyMissing(arg0.clone()),
            Self::FilesystemConflict => Self::FilesystemConflict,
            Self::GitError(arg0) => Self::GitError(git2::Error::new(arg0.code(), arg0.class(), arg0.message())),
            Self::GitObjectMissing => Self::GitObjectMissing,
            Self::ImpossibleLogic => Self::ImpossibleLogic,
            Self::IntegrityError => Self::IntegrityError,
            Self::InvalidConfig => Self::InvalidConfig,
            Self::IoError(arg0) => Self::IoError(std::io::Error::from(arg0.kind())),
            Self::NixErrno(arg0) => Self::NixErrno(*arg0),
            Self::ProcError(arg0) => Self::Collapsed(format!("From Proc Error: {}", arg0)),
            Self::ThreadFailure(arg0) => Self::Collapsed(format!("From Thread Failure: {:?}", arg0)),
            Self::TimeError(arg0) => Self::Collapsed(format!("From Time Error: {}", arg0)),
            Self::UreqError(arg0) => Self::Collapsed(format!("From Ureq Error: {}", arg0)),
            Self::UrlParseError(arg0) => Self::UrlParseError(arg0.clone()),
        }
    }
}