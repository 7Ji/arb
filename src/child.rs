use std::{collections::HashMap, ffi::OsStr, fmt::Display, fs::File, io::{stdin, BufRead, BufReader, BufWriter, Read, Write}, path::{Path, PathBuf}, process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio}, sync::{Arc, Mutex, RwLock}, thread::JoinHandle, time::Instant};

use nix::{libc::pid_t, sys::{signal::{kill, Signal}, wait::{waitpid, WaitPidFlag, WaitStatus}}, unistd::Pid, NixPath};

use crate::{filesystem::{dir_entry_checked, dir_entry_metadata_checked, file_open_append, file_open_checked, read_dir_checked}, io::{prefixed_reader_to_shared_writer, reader_to_buffer}, logfile::LogFile, Error, Result};

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
    time_start: Instant,
    log_file: Arc<Mutex<BufWriter<File>>>,
    path_log_file: PathBuf,
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
        let file_cloned = file.clone();
        let logger_stderr = std::thread::spawn(move||{
            prefixed_reader_to_shared_writer(child_err, file_cloned, "err", time_start)
        });
        Ok(Self {
            child_id: pid_from_child(child),
            time_start,
            log_file: file,
            path_log_file: logfile.path,
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
        let mut writer = match self.log_file.lock() {
            Ok(writer) => writer,
            Err(_) => {
                log::error!("Failed to get log writer for child {}", self.child_id);
                return Err(Error::ThreadFailure(None))
            },
        };
        let elapsed = (Instant::now() - self.time_start).as_secs_f64();
        if let Err(e) = writer.write_fmt(
            format_args!("[{:12.6}/---] --- end of log file ---\n", elapsed)
        ) {
            log::error!("Failed to write end-of-file to log file '{}': {}",
                self.path_log_file.display(), e);
            Err(e.into())
        } else {
            r
        }
    }
}

fn try_get_cmd_from_pid<P: AsRef<Path>>(proc: P, pid: Pid) 
    -> Result<Vec<String>> 
{
    let buffer = reader_to_buffer(
        file_open_checked(
                proc.as_ref().join(
                        format!("{}/cmdline", pid.as_raw())))?)?;
    Ok(buffer
        .split(|byte| *byte == 0)
        .map(|bytes|
            String::from_utf8_lossy(bytes).into())
        .collect())
}

fn get_cmd_from_pid<P: AsRef<Path>>(proc: P, pid: Pid) 
    -> Vec<String>
{
    match try_get_cmd_from_pid(proc, pid) {
        Ok(cmd) => cmd,
        Err(_) => vec!["--unknown--".into()],
    }
}


/// Kill all children by iterating through /proc
pub(crate) fn kill_children<P: AsRef<Path>>(proc: P) -> Result<()> {
    let mut met_children;
    let mut children_alive = HashMap::new();
    let mut proc = proc.as_ref();
    if proc.is_empty() {
        proc = "/proc".as_ref()
    }
    loop {
        met_children = false;
        for entry in read_dir_checked(proc)? {
            let entry = dir_entry_checked(entry)?;
            let file_name = entry.file_name();
            if file_name.is_empty() {
                continue
            }
            let file_name_bytes = file_name.as_encoded_bytes();
            if file_name_bytes[0] < b'0' || file_name_bytes[0] > b'9' {
                continue
            }
            let metadata = dir_entry_metadata_checked(&entry)?;
            if ! metadata.is_dir() {
                continue
            }
            let pid = match file_name.to_str() {
                Some(file_name) => match file_name.parse() {
                    Ok(pid) => {
                        if pid <= 1 {
                            continue
                        }
                        Pid::from_raw(pid)
                    },
                    Err(e) => {
                        log::warn!("Failed to parse /proc entry '{}' to pid: {}", 
                            file_name, e);
                        continue;
                    },
                }
                None => continue,
            };
            met_children = true;
            log::debug!("Waiting child {}, nohang", pid);
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(status) => {
                    if let Some(pid_waited) = status.pid() {
                        if pid_waited != pid {
                            log::error!("Waited PID {} is not the one we \
                                waited {}", pid_waited, pid);
                            return Err(Error::BadChild { 
                                pid: Some(pid_waited), code: None })
                        }
                    }
                    match status {
                        WaitStatus::Exited(_, code) => {
                            if code != 0 {
                                log::warn!("Child {} exited with error: {}", 
                                    pid, code)
                            }
                            continue
                        },
                        WaitStatus::Signaled(_, signal, coredump) 
                        => {
                            log::warn!("Child {} exited with signal {}, \
                                coredump: {}", pid, signal, coredump);
                            continue
                        },
                        WaitStatus::Stopped(_, signal) =>
                            log::warn!("Child {} stopped with signal {}",
                                        pid, signal),
                        WaitStatus::PtraceEvent(_, signal, event) 
                        => log::warn!("Child {} stopped with ptrace event \
                            signal {} id {}", pid, signal, event),
                        WaitStatus::PtraceSyscall(_) => 
                            log::warn!("Child {} stopped with ptrace syscall", 
                                pid),
                        WaitStatus::Continued(_) => {
                            log::error!("Child {} continued but we shouldn't \
                                get this status", pid);
                            return Err(Error::BadChild { 
                                pid: Some(pid), code: None })
                        },
                        WaitStatus::StillAlive => (),
                    }
                },
                Err(e) => {
                    log::error!("Failed to wait for child {}: {}", pid, e);
                    return Err(e.into())
                },
            }
            // We now only have naughty children that did not want to die
            // with their parent. Kill them, soft first, hard then
            match children_alive.get_mut(&pid) {
                Some(count) => if *count > 1 {
                    log::error!("Child {} still remains after we sent it \
                        SIGKILL", pid);
                    return Err(Error::BadChild { pid: Some(pid), code: None })

                } else {
                    if let Err(e) =  kill(pid, Signal::SIGKILL) {
                        log::error!("Failed to send SIGKILL to child {}: {}", pid, e);
                        return Err(Error::BadChild { pid: Some(pid), code: None })
                    }
                    log::warn!("Sent SIGKILL to child {} ({:?})", pid, 
                        get_cmd_from_pid(proc, pid));
                    *count += 1
                },
                None => {
                    if let Err(e) =  kill(pid, Signal::SIGTERM) {
                        log::error!("Failed to send SIGTERM to child {}: {}", pid, e);
                        return Err(Error::BadChild { pid: Some(pid), code: None })
                    }
                    log::warn!("Sent SIGTERM to child {} ({:?})", pid,
                        get_cmd_from_pid(proc, pid));
                    children_alive.insert(pid, 1);
                },
            }
        }
        if ! met_children {
            break
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Ok(())
}