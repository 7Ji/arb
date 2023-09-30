pub(super) const BUFFER_SIZE: usize = 0x400000; // 4M

pub(super) fn wait_child(mut child: std::process::Child, job: &str) 
    -> Result<(), ()> 
{
    let status = match child.wait() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to wait for child to {}: {}", &job, e);
            return Err(())
        },
    };
    match status.code() {
        Some(code) => {
            if code == 0 {
                return Ok(())
            } else {
                eprintln!("Child to {} bad return {}", &job, code);
                return Err(())
            }
        },
        None => {
            eprintln!("Child to {} has no return", &job);
            return Err(())
        },
    }
}

pub(super) fn spawn_and_wait(command: &mut std::process::Command, job: &str)
    -> Result<(), ()> 
{
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!(
                "Failed to spawn child to {}: {}", &job, e);
            return Err(())
        },
    };
    wait_child(child, job)
}