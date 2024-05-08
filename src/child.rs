use std::{ffi::OsStr, process::Child};

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