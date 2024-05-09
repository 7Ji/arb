use std::{ffi::OsStr, io::Write, process::{Child, Command, Stdio}};

use nix::{libc::pid_t, unistd::Pid};

use crate::{Error, Result};

pub(crate) fn wait_child(child: &mut Child) -> Result<()> {
    match child.wait() {
        Ok(status) => if status.success() {
            Ok(())
        } else {
            log::error!("Child {} bad return {}", child.id(), status);
            Err(Error::BadChild { 
                pid: Some(Pid::from_raw(child.id() as pid_t)), 
                code: status.code() })
        },
        Err(e) => {
            log::error!("Failed to wait for child: {}", e);
            if let Err(e) = child.kill() {
                log::error!("Failed to kill failed child: {}", e);
            }
            Err(e.into())
        },
    }
}

pub(crate) fn command_new_no_stdin<S: AsRef<OsStr>>(exe: S) -> Command {
    let mut command = Command::new(exe);
    command.stdin(Stdio::null());
    command
}

pub(crate) fn write_to_child<B: AsRef<[u8]>>(child: &mut Child, content:B) 
    -> Result<()> 
{
    let mut child_in = match child.stdin.take() {
        Some(child_in) => child_in,
        None => {
            return Err(Error::BadChild { 
                pid: Some(Pid::from_raw(child.id() as pid_t)), 
                code: None })
        },
    };
    let content = content.as_ref();
    if let Err(e) = child_in.write_all(content) {
        log::error!("Failed to write {} bytes into child {}: {}",
            content.len(), child.id(), e);
        Err(e.into())
    } else {
        Ok(())
    }
}