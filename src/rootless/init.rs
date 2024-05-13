use std::{ffi::OsString, io::{stdin, Write}, path::{Path, PathBuf}, process::Child};

use nix::{errno::Errno, libc::pid_t, sys::wait::{wait, WaitStatus}, unistd::{chroot, Pid}};
use serde::{Deserialize, Serialize};

use crate::{child::command_new_no_stdin, filesystem::set_current_dir_checked, logfile::LogFile, mount::mount_proc, Error, Result};

use super::{action::run_action_stateless, root::chroot_checked};

#[derive(Serialize, Deserialize)]
pub(crate) enum InitCommand {
    /// Run applet of self, again. Note this would usually be impossible after 
    /// `Chroot`, as our own executable would be impossible to look up
    RunApplet { 
        applet: OsString,
        args: Vec<OsString>,
    },
    RunProgram {
        logfile: OsString,
        program: OsString,
        args: Vec<OsString>,
    },
    MountProc {
        path: OsString
    },
    Chdir {
        path: OsString,
    },
    Chroot {
        path: OsString,
    },
}

impl InitCommand {
    fn work(self) -> Result<()> {
        match self {
            InitCommand::RunApplet { applet, args } => {
                log::debug!("Running applet '{}' with args {:?}", 
                    applet.to_string_lossy(), args);
                run_action_stateless::<&Path, _, _, _, &[u8]>(
                    None, applet, args, None)
            },
            InitCommand::RunProgram { logfile, program, args } => {
                log::debug!("Running program '{}' with args {:?}", 
                    program.to_string_lossy(), args);
                let mut command = command_new_no_stdin(&program);
                if ! logfile.is_empty() {
                    let logfile = LogFile::try_from(logfile)?;
                    log::info!("Program log file: '{}' => '{}'",
                        program.to_string_lossy(), logfile.path.display());
                    let (child_out, child_err) = logfile.try_split()?;
                    command.stdout(child_out)
                        .stderr(child_err);
                }
                let child = match command
                    .env_clear()
                    .env("LANG", "en_US.UTF-8") // 7Ji: No en_US
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
                wait_all(child)
            },
            InitCommand::MountProc { path } => {
                log::debug!("Mounting proc to '{}'", path.to_string_lossy());
                mount_proc(path)
            },
            InitCommand::Chdir { path } => {
                log::debug!("Changing workdir to '{}'", path.to_string_lossy());
                set_current_dir_checked(path)
            },
            InitCommand::Chroot { path } => {
                log::debug!("Chrooting to '{}'", path.to_string_lossy());
                chroot_checked(path)
            },
        }
    }
}

/// An internal struct carrying instructions, passed from parent into child's
/// stdin
/// 
#[derive(Serialize, Deserialize)]
pub(crate) struct InitPayload {
    commands: Vec<InitCommand>
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

    pub(crate) fn add_command(&mut self, command: InitCommand) {
        self.commands.push(command)
    }

    pub(crate) fn add_command_run_program<S1, S2, I, S3>(
        &mut self, logfile: S1, program: S2, args: I
    ) 
    where
        S1: Into<OsString>,
        S2: Into<OsString>,
        I: IntoIterator<Item = S3>,
        S3: Into<OsString>
    {
        let logfile = logfile.into();
        let program = program.into();
        let args = args.into_iter().map(
            |arg|arg.into()).collect();
        self.commands.push(InitCommand::RunProgram { 
            logfile, program, args })
    }

    pub(crate) fn add_command_run_applet<S1, I, S2>(
        &mut self, applet: S1, args: I
    ) 
    where
        S1: Into<OsString>,
        I: IntoIterator<Item = S2>,
        S2: Into<OsString>
    {
        let applet = applet.into();
        let args = args.into_iter().map(
            |arg|arg.into()).collect();
        self.commands.push(InitCommand::RunApplet { 
            applet, args })
    }
}

/// A dump init implementation that wait for all children
fn wait_all(child: Child) -> Result<()> {
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