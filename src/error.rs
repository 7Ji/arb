#[derive(Clone, Debug)]
pub(crate) enum Error {
    // AlpmError (alpm::Error),
    AlnopmError (alnopm::Error),
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
    /// Collapsed git error
    GitError (String),
    GitObjectMissing,
    IllegalWorkerState (&'static str),
    ImpossibleLogic,
    IntegrityError,
    InvalidArgument,
    InvalidConfig,
    IoError (String),
    MappingFailure,
    NixErrno (nix::errno::Errno),
    PkgbuildLibError (pkgbuild::Error),
    /// Collapse rmp decode error
    RmpDecodeError (String),
    /// Collapse rmp encode error
    RmpEncodeError (String),
    // ProcError (procfs::ProcError),
    ThreadFailure,
    // TimeError (time::Error),
    /// Collapsed ureq error
    UreqError (String),
    /// Collapsed url parsing error
    UrlParseError (url::ParseError),
    /// Collapsed YAML parse error
    YAMLParseError (String),
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
            Self::AlnopmError(e) => write!(f, "Alnopm library error: {}", e),
            // Self::AlpmError(e) => write!(f, "Alpm Error: {}", e),
            Self::BadChild { pid, code } => write!(f, "Bad child, pid {:?}, code {:?}", pid, code),
            Self::BrokenEnvironment => write!(f, "Broken Environment"),
            Self::BrokenPKGBUILDs(pkgbuilds) => write!(f, "Broken PKGBUILDs: {:?}", pkgbuilds),
            Self::BuildFailure => write!(f, "Build Failure"),
            Self::Collapsed(s) => write!(f, "Collapsed {}", s),
            Self::DependencyMissing( deps ) => write!(f, "Dependency missing: {:?}", deps),
            Self::FilesystemConflict => write!(f, "Filesystem Conflict"),
            Self::GitObjectMissing => write!(f, "Git Object Missing"),
            Self::GitError(e) => write!(f, "Git Error: {}", e),
            Self::ImpossibleLogic => write!(f, "Impossible Logic"),
            Self::IntegrityError => write!(f, "Integrity Error"),
            Self::InvalidArgument => write!(f, "Invalid Argument"),
            Self::InvalidConfig => write!(f, "Invalid Config"),
            Self::IoError(e) => write!(f, "IO Error: {}", e),
            Self::IllegalWorkerState(s) => write!(f, "Illegal Worker State: {}", s),
            Self::MappingFailure => write!(f, "Mapping Failure"),
            Self::NixErrno(e) => write!(f, "Nix Errno: {}", e),
            Self::PkgbuildLibError(e) => write!(f, "PKGBUILD Library Error: {}", e),
            Self::RmpDecodeError(e) => write!(f, "RMP Decode Error: {}", e),
            Self::RmpEncodeError(e) => write!(f, "RMP Encode Error: {}", e),
            // Self::ProcError(e) => write!(f, "Proc Error: {}", e),
            Self::ThreadFailure => write!(f, "Thread Failure"),
            // Self::TimeError(e) => write!(f, "Time Error: {}", e),
            Self::UreqError(e) => write!(f, "Ureq Error: {}", e),
            Self::UrlParseError(e) => write!(f, "URL Parse Error: {}", e),
            Self::YAMLParseError(e) => write!(f, "YAML Parse Error: {}", e),
        }
    }
}

macro_rules! impl_from_error_move {
    ($external: ty, $internal: tt) => {
        impl From<$external> for Error {
            fn from(value: $external) -> Self {
                Self::$internal(value)
            }
        }
    };
}

macro_rules! impl_from_error_collapse {
    ($external: ty, $internal: tt) => {
        impl From<&$external> for Error {
            fn from(value: &$external) -> Self {
                Self::$internal(format!("{}", value))
            }
        }
        impl From<$external> for Error {
            fn from(value: $external) -> Self {
                Self::$internal(format!("{}", value))
            }
        }
    };
}

impl_from_error_move!(alnopm::Error, AlnopmError);
impl_from_error_collapse!(git2::Error, GitError);
impl_from_error_collapse!(std::io::Error, IoError);
impl_from_error_move!(nix::errno::Errno, NixErrno);
impl_from_error_collapse!(ureq::Error, UreqError);
impl_from_error_move!(url::ParseError, UrlParseError);
impl_from_error_collapse!(serde_yaml::Error, YAMLParseError);
impl_from_error_collapse!(rmp_serde::decode::Error, RmpDecodeError);
impl_from_error_collapse!(rmp_serde::encode::Error, RmpEncodeError);
impl_from_error_move!(pkgbuild::Error, PkgbuildLibError);

impl From<Box<dyn std::any::Any + Send + 'static>> for Error {
    fn from(_: Box<dyn std::any::Any + Send + 'static>) -> Self {
        Self::ThreadFailure
    }
}