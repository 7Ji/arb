pub(crate) struct ForkedChild {
    pub(crate) pid: libc::pid_t
}

impl ForkedChild {
    pub(crate) fn wait(&self) -> Result<(), ()> {
        let mut status: libc::c_int = 0;
        let waited_pid = unsafe {
            libc::waitpid(self.pid, &mut status, 0)
        };
        if waited_pid <= 0 {
            eprintln!("Failed to wait for child: {}", 
                std::io::Error::last_os_error());
            return Err(())
        }
        if waited_pid != self.pid {
            eprintln!("Waited child {} is not the child {} we forked", 
                        waited_pid, self.pid);
            return Err(())
        }
        if status != 0 {
            eprintln!("Child process failed");
            return Err(())
        }
        Ok(())
    }

    pub(crate) fn wait_noop(&self) -> Result<Option<Result<(), ()>>, ()> {
        let mut status: libc::c_int = 0;
        let waited_pid = unsafe {
            libc::waitpid(self.pid, &mut status, libc::WNOHANG)
        };
        if waited_pid < 0 {
            eprintln!("Failed to wait for child: {}", 
                std::io::Error::last_os_error());
            return Err(())
        } else if waited_pid == 0 {
            return Ok(None)
        }
        if waited_pid != self.pid {
            eprintln!("Waited child {} is not the child {} we forked", 
                        waited_pid, self.pid);
            return Err(())
        }
        if status != 0 {
            eprintln!("Child process failed");
            return Ok(Some(Err(())))
        }
        Ok(Some(Ok(())))
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
                        eprintln!("Child {} bad return {}", &job, code);
                        Err(())
                    },
                None => {
                    eprintln!("Failed to get return code of child {}", &job);
                    Err(())
                },
            }
        },
        Err(e) => {
            eprintln!("Failed to spawn child to {}: {}", &job, e);
            Err(())
        },
    }
}