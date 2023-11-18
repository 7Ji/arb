pub(crate) enum Error {
    ThreadFailure (Option<Box<dyn std::any::Any + Send + 'static>>),
    IoError (std::io::Error),
    FilesystemConflict,
    ImpossibleLogic,
    BadChild {
        pid: Option<nix::unistd::Pid>,
        code: Option<i32>,
    }
}

pub(crate) type Result<T> = std::result::Result<T, Error>;