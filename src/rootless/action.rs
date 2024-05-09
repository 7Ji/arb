
use std::{ffi::OsStr, process::Child};

use crate::{child::{command_new_no_stdin, wait_child}, Error, Result};

use super::arg0::get_arg0;

pub(crate) fn start_action<S1, S2, I1, S3>(
    program: Option<S1>, applet: S2, main_args: I1
) -> Result<Child> 
where
    S1: AsRef<OsStr>,
    S2: AsRef<OsStr>,
    I1: IntoIterator<Item = S3>,
    S3: AsRef<OsStr>
{
    let mut command = match program {
        Some(program) => command_new_no_stdin(program),
        None => command_new_no_stdin(get_arg0()),
    };
    match command.arg(&applet).args(main_args).spawn() 
    {
        Ok(child) => Ok(child),
        Err(e) => {
            log::error!("Failed to run applet '{}'", 
                        applet.as_ref().to_string_lossy());
            return Err(e.into())
        },
    }
}
pub(crate) fn run_stateless<S1, S2, I1, S3>(
    program: Option<S1>, applet: S2, main_args: I1
) -> Result<()> 
where
    S1: AsRef<OsStr>,
    S2: AsRef<OsStr>,
    I1: IntoIterator<Item = S3>,
    S3: AsRef<OsStr>
{
    wait_child(&mut start_action(program, applet, main_args)?)
}
