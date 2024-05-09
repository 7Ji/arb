use std::{io::{stdin, Write}, process::Child};

use nix::{errno::Errno, libc::pid_t, sys::wait::{wait, WaitStatus}, unistd::Pid};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Serialize, Deserialize)]
pub(crate) enum InitCommand {
    Run {
        program: String,
        args: Vec<String>,
    },
    MountProc,

}

impl InitCommand {
    fn work(self) -> Result<()> {
        match self {
            InitCommand::Run { program, args } => todo!(),
            InitCommand::MountProc => todo!(),
        }
    }
}

/// An internal struct carrying instructions, passed from parent into child's
/// stdin
/// 
#[derive(Serialize, Deserialize)]
pub(crate) struct InitPayload {
    pub(crate) commands: Vec<InitCommand>
}

impl InitPayload {
    pub(crate) fn try_read() -> Result<Self> {
        let payload = rmp_serde::from_read(stdin())?;
        Ok(payload)
    }

    pub(crate) fn try_write<W: Write>(&self, mut writer: W) -> Result<()> {
        let value = rmp_serde::to_vec(self)?;
        writer.write_all(&value)?;
        Ok(())
    }

    pub(crate) fn work(self) -> Result<()> {
        for command in self.commands {
            command.work()?
        }
        Ok(())
    }
}

/// A dump init implementation that reaps dead children endlessly
fn reaper(child: Child) -> Result<()> {
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