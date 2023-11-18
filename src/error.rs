
#[derive(Debug)]
pub(crate) enum Error {
    AlpmError (alpm::Error),
    BadChild {
        pid: Option<nix::unistd::Pid>,
        code: Option<i32>,
    },
    FilesystemConflict,
    ImpossibleLogic,
    InvalidConfig,
    IoError (std::io::Error),
    NixErrno (nix::errno::Errno),
    ProcError (procfs::ProcError),
    ThreadFailure (Option<Box<dyn std::any::Any + Send + 'static>>),
    UreqError (ureq::Error),
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::AlpmError(e) => write!(f, "Alpm Error: {}", e),
            Error::BadChild { pid, code } => write!(f, "Bad child, pid {:?}, code {:?}", pid, code),
            Error::FilesystemConflict => write!(f, "Filesystem Conflict"),
            Error::ImpossibleLogic => write!(f, "Impossible Logic"),
            Error::InvalidConfig => write!(f, "Invalid Config"),
            Error::IoError(e) => write!(f, "IO Error: {}", e),
            Error::NixErrno(e) => write!(f, "Nix Errno: {}", e),
            Error::ProcError(e) => write!(f, "Proc Error: {}", e),
            Error::ThreadFailure(artifact) => write!(f, "Thread Failure, artifact: {:?}", artifact),
            Error::UreqError(e) => write!(f, "Ureq Error: {}", e),
        }
    }
}