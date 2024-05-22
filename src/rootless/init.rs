use std::{collections::HashMap, ffi::OsString, io::stdin, path::{Path, PathBuf}, process::Child};

use nix::{errno::Errno, sys::wait::{wait, WaitStatus}, NixPath};
use serde::{Deserialize, Serialize};

use crate::{child::{command_new_no_stdin, command_new_no_stdin_with_piped_out_err, kill_children, pid_from_child, ChildLoggers}, filesystem::set_current_dir_checked, logfile::LogFile, mount::mount_proc, Error, Result};

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

struct InitCache {
    proc: PathBuf,
    logfiles: HashMap<OsString, LogFile>,
}

impl InitCommand {
    fn work(self, cache: &mut InitCache) -> Result<()> {
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
                let mut child = match if logfile.is_empty() {
                    command_new_no_stdin(&program)
                } else {
                    command_new_no_stdin_with_piped_out_err(&program)
                }
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
                let mut child_loggers = None;
                if ! logfile.is_empty() {
                    log::info!("Program log file: '{}' => '{}'",
                        program.to_string_lossy(), logfile.to_string_lossy());
                    if let Some(logfile) = cache.logfiles.remove(&logfile) {
                        child_loggers = Some(ChildLoggers::try_new(&mut child, logfile)?)
                    } else {
                        log::warn!("Failed to find cached log file but \
                            requested to write to log file, redirecting to \
                            our own stdout & stderr");
                    }
                }
                wait_any_till(child)?;
                if let Some(child_loggers) = child_loggers {
                    child_loggers.try_join()?
                }
                Ok(())
            },
            InitCommand::MountProc { path } => {
                log::debug!("Mounting proc to '{}'", path.to_string_lossy());
                mount_proc(&path)?;
                cache.proc = path.into();
                Ok(())
            },
            InitCommand::Chdir { path } => {
                log::debug!("Changing workdir to '{}'", path.to_string_lossy());
                set_current_dir_checked(path)
            },
            InitCommand::Chroot { path } => {
                log::debug!("Chrooting to '{}'", path.to_string_lossy());
                set_current_dir_checked(&path)?;
                chroot_checked(".")?;
                if ! cache.proc.is_empty() {
                    cache.proc = "/proc".into();
                }
                Ok(())
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

    fn try_cache(&self) -> Result<InitCache> {
        let mut logfiles = HashMap::new();
        for command in self.commands.iter() {
            if let InitCommand::RunProgram { 
                logfile, program: _, args: _ } = command 
            {
                if logfile.is_empty() {
                    continue
                }
                if logfiles.insert(
                    logfile.clone(), 
                    LogFile::try_open(logfile)?
                ).is_some() {
                    log::warn!("Duplicated log file '{}'", 
                            logfile.to_string_lossy());
                }
            }
        }
        Ok(InitCache { logfiles, proc: PathBuf::new() })
    }

    pub(crate) fn work(self) -> Result<()> {
        let mut cache = self.try_cache()?;
        for command in self.commands {
            command.work(&mut cache)?
        }
        kill_children(&cache.proc)?;
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

/// A dump init implementation that wait for any children until a specific
/// child exited
fn wait_any_till(child: Child) -> Result<()> {
    let pid_direct = pid_from_child(&child);
    let mut code = None;
    loop {
        match wait() {
            Ok(r) => 
                if let WaitStatus::Exited(pid, code_this) = r {
                    if pid == pid_direct {
                        code = Some(code_this); 
                        // We will send out SIGCHILD to all children when we
                        // quit, Just let these children die (especially
                        // gpg-agent)
                        break
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