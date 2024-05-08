use std::process::Child;

use nix::{errno::Errno, libc::pid_t, sys::wait::{wait, WaitStatus}, unistd::Pid};

use crate::{Error, Result};

/// A dump init implementation that reaps dead children endlessly
pub(crate) fn reaper(child: Child) -> Result<()> {
    let pid_direct = Pid::from_raw(child.id() as pid_t);
    let mut code = None;
    loop {
        match wait() {
            Ok(r) => 
                if let WaitStatus::Exited(pid, code_this) = r {
                    if pid == pid_direct {
                        code = Some(code_this)
                    }
                },
            Err(e) =>
                if e == Errno::ECHILD { // Only break when there's no child
                    break
                } else {
                    log::error!("Failed to wait: {}", e);
                    return Err(e.into())
                }
        }
    }
    if Some(0) == code {
        Ok(())
    } else {
        log::error!("Direct child either exited abnormally or was not catched, \
            code: {:?}", code);
        let pid = Some(pid_direct);
        Err(Error::BadChild { pid, code })
    }
}