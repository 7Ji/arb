use nix::{
        unistd::Pid,
        sys::wait::{
            waitpid,
            WaitPidFlag,
            WaitStatus,
        }
    };

pub(crate) struct ForkedChild {
    pub(crate) pid: Pid
}

impl ForkedChild {
    pub(crate) fn wait(&self) -> Result<(), ()> {
        match waitpid(self.pid, None) {
            Ok(status) => match status {
                WaitStatus::Exited(pid, code) =>
                    if pid == self.pid {
                        if code == 0 {
                            return Ok(())
                        } else {
                            log::error!("Child {} non-zero exit code {}",
                                self.pid, code)
                        }
                    } else {
                        log::error!("Waited {} is not our child {}, its exit code
                            {}", pid, self.pid, code)
                    }
                _ => log::error!("Child {} did not exit cleanly: {:?}",
                        self.pid, status)
            },
            Err(e) =>
                log::error!("Failed to wait for child {}: {}", self.pid, e),
        }
        Err(())
    }

    pub(crate) fn wait_noop(&self) -> Result<Option<Result<(), ()>>, ()> {
        match waitpid(self.pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status) => match status {
                WaitStatus::StillAlive => Ok(None),
                WaitStatus::Exited(pid, code) =>
                    if pid == self.pid {
                        if code == 0 {
                            Ok(Some(Ok(())))
                        } else {
                            log::error!("Child {} non-zero exit code {}",
                                self.pid, code);
                            Ok(Some(Err(())))
                        }
                    } else {
                        log::error!("Waited {} is not our child {}, its exit code
                            {}", pid, self.pid, code);
                        Ok(Some(Err(())))
                    }
                _ => {
                    log::error!("Child {} did not exit cleanly: {:?}",
                        self.pid, status);
                    Ok(Some(Err(())))
                }
            },
            Err(e) => {
                log::error!("Failed to wait for child {}: {}", self.pid, e);
                Err(())
            },
        }
    }
}



pub(crate) fn output_and_check(command: &mut std::process::Command, job: &str)
    -> Result<(), ()>
{
    match command.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(output) => {
            match output.status.code() {
                Some(code) =>
                    if code == 0 {
                        Ok(())
                    } else {
                        log::error!("Child {} bad return {}", &job, code);
                        Err(())
                    },
                None => {
                    log::error!("Failed to get return code of child {}", &job);
                    Err(())
                },
            }
        },
        Err(e) => {
            log::error!("Failed to spawn child to {}: {}", &job, e);
            Err(())
        },
    }
}