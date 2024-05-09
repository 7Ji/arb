
use std::{ffi::OsStr, iter::empty, process::{Child, Command, Stdio}};

use crate::{child::{command_new_no_stdin, wait_child, write_to_child}, Error, Result};

use super::arg0::get_arg0;

pub(crate) fn start_action<S1, S2, I, S3>(
    program: Option<S1>, applet: S2, args: I, no_stdin: bool
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
    if no_stdin {
        command.stdin(Stdio::null());
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

pub(crate) fn run_stateless<S1, S2, I, S3, B>(
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
        program, applet, args, payload.is_none())?;
    if let Some(payload) = payload {
        write_to_child(&mut child, payload)?
    }
    wait_child(&mut child)
}

// pub(crate) fn run_stateless_no_arg<S1, S2>(
//     program: Option<S1>, applet: S2
// ) -> Result<()> 
// where
//     S1: AsRef<OsStr>,
//     S2: AsRef<OsStr>,
// {
//     let mut child = start_action::<_, _, _, &str>(
//         program, applet, empty(), true)?;
//     wait_child(&mut child)
// }

// pub(crate) fn run_stateless_with_payload<S1, S2, I1, S3, B>(
//     program: Option<S1>, applet: S2, main_args: I1, payload: B
// ) -> Result<()> 
// where
//     S1: AsRef<OsStr>,
//     S2: AsRef<OsStr>,
//     I1: IntoIterator<Item = S3>,
//     S3: AsRef<OsStr>,
//     B: AsRef<[u8]>
// {
//     let mut child = start_action(
//         program, applet, main_args, false)?;
//     write_to_child(&mut child, payload.as_ref())?;
//     wait_child(&mut child)
// }

// pub(crate) fn run_stateless_no_arg_with_payload<S1, S2>(
//     program: Option<S1>, applet: S2
// ) -> Result<()> 
// where
//     S1: AsRef<OsStr>,
//     S2: AsRef<OsStr>,
// {
//     let mut child = start_action::<_, _, _, &str>(
//         program, applet, empty(), true)?;
//     wait_child(&mut child)
// }
