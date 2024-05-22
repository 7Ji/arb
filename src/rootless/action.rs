
use std::{ffi::OsStr, iter::empty, path::Path, process::{Child, Command, Stdio}};

use crate::{child::{wait_child, write_to_child}, Result};

use super::arg0::get_arg0;

pub(crate) fn start_action<S1, S2, I, S3>(
    program: Option<S1>, applet: S2, args: I, pipe_in: bool, pipe_out: bool
) -> Result<Child> 
where
    S1: AsRef<OsStr>,
    S2: AsRef<OsStr>,
    I: IntoIterator<Item = S3>,
    S3: AsRef<OsStr>
{
    let mut command = match program {
        Some(program) => Command::new(program),
        None => Command::new(get_arg0()),
    };
    command.stdin(if pipe_in { Stdio::piped() } else { Stdio::null() });
    if pipe_out {
        command.stdout(Stdio::piped());
    }
    match command.arg(&applet).args(args).spawn() 
    {
        Ok(child) => Ok(child),
        Err(e) => {
            log::error!("Failed to run applet '{}'", 
                        applet.as_ref().to_string_lossy());
            return Err(e.into())
        },
    }
}

pub(crate) fn run_action_stateless<S1, S2, I, S3, B>(
    program: Option<S1>, applet: S2, args: I, payload: Option<B>
) -> Result<()> 
where
    S1: AsRef<OsStr>,
    S2: AsRef<OsStr>,
    I: IntoIterator<Item = S3>,
    S3: AsRef<OsStr>,
    B: AsRef<[u8]>
{
    let mut child = start_action(
        program, applet, args, payload.is_some(), false)?;
    if let Some(payload) = payload {
        write_to_child(&mut child, payload)?
    }
    wait_child(&mut child)
}

pub(crate) fn run_action_stateless_no_program_no_args<S, B>(
    applet: S, payload: Option<B>
) -> Result<()> 
where
    S: AsRef<OsStr>,
    B: AsRef<[u8]>
{
    run_action_stateless::<&Path, _, _, &str, _>(
        None, applet, empty(), payload)
}