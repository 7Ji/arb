pub(super) fn output_and_check(command: &mut std::process::Command, job: &str)
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