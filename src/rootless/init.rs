use std::{ffi::OsString, io::{stdin, Write}, path::{Path, PathBuf}, process::Child};

use nix::{errno::Errno, libc::pid_t, sys::wait::{wait, WaitStatus}, unistd::Pid};
use serde::{Deserialize, Serialize};

use crate::{child::command_new_no_stdin, mount::mount_proc, Error, Result};

#[derive(Serialize, Deserialize)]
pub(crate) enum InitCommand {
    RunProgram {
        program: OsString,
        args: Vec<OsString>,
    },
    MountProc {
        path: OsString
    },
}

impl InitCommand {
    fn work(self) -> Result<()> {
        match self {
            InitCommand::RunProgram { program, args } => {
                let child = match command_new_no_stdin(&program)
                    .args(args)
                    .spawn() 
                {
                    Ok(child) => child,
                    Err(e) => {
                        log::error!("Failed to spawn child '{}': {}", 
                                    program.to_string_lossy(), e);
                        return Err(e.into())
                    },
                };
                reaper(child)
            },
            InitCommand::MountProc { path } => mount_proc(path),
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
    pub(crate) fn new_with_root<P: AsRef<Path>>(root: P) -> Self {
        Self {
            commands: vec![InitCommand::MountProc { 
                path: root.as_ref().join("proc").into_os_string()
            }],
        }
    }

    pub(crate) fn try_read() -> Result<Self> {
        match rmp_serde::from_read(stdin()) {
            Ok(payload) => Ok(payload),
            Err(e) => {
                log::error!("Failed to deserialize init payload from stdin: \
                            {}", e);
                Err(e.into())
            },
        }
    }

    pub(crate) fn try_into_bytes(&self) -> Result<Vec<u8>> {
        match rmp_serde::to_vec(self) {
            Ok(bytes) => Ok(bytes),
            Err(e) => {
                log::error!("Failed to serialize init payload to bytes: {}", 
                            e);
                Err(e.into())
            },
        }
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