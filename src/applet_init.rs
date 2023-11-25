use std::ffi::OsString;

use crate::error::{
        Error,
        Result
    };


pub(crate) fn main<I, S>(_args: I) -> Result<()>
where
    I: Iterator<Item = S>,
    S: Into<OsString> + Clone,
{
    // unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID).unwrap();
    // wait_root();
    // let args = Args::parse_from(args);
    // let mut command = std::process::Command::new("/bin/bash");
    // if ! args.command.is_empty() {
    //     command.arg("-c")
    //         .arg(args.command);
    // };
    // command
    //     .spawn()
    //     .unwrap()
    //     .wait()
    //     .unwrap();
    Err(Error::ImpossibleLogic)
}