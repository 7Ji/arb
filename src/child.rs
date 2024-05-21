use std::{ffi::OsStr, fmt::Display, io::{stdin, BufRead, BufReader, BufWriter, Read, Write}, process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio}, sync::{Arc, Mutex, RwLock}, thread::JoinHandle, time::Instant};

use nix::{libc::pid_t, unistd::Pid};

use crate::{io::prefixed_reader_to_shared_writer, logfile::LogFile, Error, Result};

pub(crate) fn pid_from_child(child: &Child) -> Pid {
    Pid::from_raw(child.id() as pid_t)
}

pub(crate) fn wait_child(child: &mut Child) -> Result<()> {
    match child.wait() {
        Ok(status) => if status.success() {
            Ok(())
        } else {
            log::error!("Child {} bad return {}", child.id(), status);
            Err(Error::BadChild { 
                pid: Some(pid_from_child(child)), 
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

pub(crate) fn command_new_no_stdin_with_piped_out_err<S: AsRef<OsStr>>(exe: S) 
-> Command {
    let mut command = command_new_no_stdin(exe);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

pub(crate) fn write_to_child<B: AsRef<[u8]>>(child: &mut Child, content:B) 
    -> Result<()> 
{
    let mut child_in = get_child_in(child)?;
    let content = content.as_ref();
    if let Err(e) = child_in.write_all(content) {
        log::error!("Failed to write {} bytes into child {}: {}",
            content.len(), child.id(), e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn read_from_child(child: &mut Child) -> Result<Vec<u8>> 
{
    let mut child_out = get_child_out(child)?;
    let mut buffer = Vec::new();
    match child_out.read_to_end(&mut buffer) {
        Ok(size) => {
            log::debug!("Read {} bytes from child {}", size, child.id());
            Ok(buffer)
        },
        Err(e) => {
            log::error!("Failed to read from child {}: {}", child.id(), e);
            Err(e.into())
        },
    }
}

pub(crate) fn get_child_in(child: &mut Child) -> Result<ChildStdin> {
    match child.stdin.take() {
        Some(child_in) => Ok(child_in),
        None => {
            log::error!("Failed to take stdin from child {}", child.id());
            Err(Error::BadChild { 
                pid: Some(pid_from_child(child)), 
                code: None })
        },
    }
}

pub(crate) fn get_child_out(child: &mut Child) -> Result<ChildStdout> {
    match child.stdout.take() {
        Some(child_out) => Ok(child_out),
        None => {
            log::error!("Failed to take stdout from child {}", child.id());
            Err(Error::BadChild { 
                pid: Some(pid_from_child(child)), 
                code: None })
        },
    }
}

pub(crate) fn get_child_err(child: &mut Child) -> Result<ChildStderr> {
    match child.stderr.take() {
        Some(child_err) => Ok(child_err),
        None => {
            log::error!("Failed to take stderr from child {}", child.id());
            Err(Error::BadChild { 
                pid: Some(pid_from_child(child)), 
                code: None })
        },
    }
}

pub(crate) fn get_child_in_out(child: &mut Child) 
    -> Result<(ChildStdin, ChildStdout)> 
{
    Ok((get_child_in(child)?, get_child_out(child)?))
}

pub(crate) fn spawn_and_wait(command: &mut Command) -> Result<()> {
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            log::error!("Failed to spawn child: {}", e);
            return Err(e.into())
        },
    };
    wait_child(&mut child)
}

pub(crate) struct ChildLoggers {
    child_id: Pid,
    logger_stdout: JoinHandle<Result<()>>,
    logger_stderr: JoinHandle<Result<()>>,
}

impl ChildLoggers {
    pub(crate) fn try_new(child: &mut Child, logfile: LogFile) -> Result<Self> {
        let child_out = get_child_out(child)?;
        let child_err = get_child_err(child)?;
        let file = Arc::new(Mutex::new(BufWriter::new(logfile.file)));
        let time_start = Instant::now();
        let file_cloned = file.clone();
        let logger_stdout = std::thread::spawn(move||{
            prefixed_reader_to_shared_writer(child_out, file_cloned, "out", time_start)
        });
        let logger_stderr = std::thread::spawn(move||{
            prefixed_reader_to_shared_writer(child_err, file, "err", time_start)
        });
        Ok(Self {
            child_id: pid_from_child(child),
            logger_stdout,
            logger_stderr
        })
    }

    pub(crate) fn try_join(self) -> Result<()> {
        let mut r = Ok(());
        if let Err(e) = self.logger_stdout.join() {
            log::error!("Failed to join stdout logger for child {}", self.child_id);
            r = Err(Error::ThreadFailure(Some(e)));
        }
        if let Err(e) = self.logger_stderr.join() {
            log::error!("Failed to join stderr logger for child {}", self.child_id);
            r = Err(Error::ThreadFailure(Some(e)));
        }
        r
    }
}

// pub(crate) struct ChildWithLoggers {
//     child: Child,
//     logger_stdout: JoinHandle<Result<()>>,
//     logger_stderr: JoinHandle<Result<()>>,
// }

// impl ChildWithLoggers {
//     fn try_new(mut child: Child, logfile: LogFile) -> Result<Self> {
//         let child_out = get_child_out(&mut child)?;
//         let child_err = get_child_err(&mut child)?;
//         let file = Arc::new(Mutex::new(BufWriter::new(logfile.file)));
//         let time_start = Instant::now();
//         let file_cloned = file.clone();
//         let logger_stdout = std::thread::spawn(move||{
//             prefixed_reader_to_shared_writer(child_out, file_cloned, "out", time_start)
//         });
//         let logger_stderr = std::thread::spawn(move||{
//             prefixed_reader_to_shared_writer(child_err, file, "err", time_start)
//         });
//         Ok(Self {
//             child,
//             logger_stdout,
//             logger_stderr,
//         })
//     }
// }