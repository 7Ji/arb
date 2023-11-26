/// Broker: set up a chroot-jail and send command downwards to other applet

use std::{ffi::OsString, time::Duration, thread::sleep};

use nix::{sched::{unshare, CloneFlags}, unistd::{getuid, getgid}};

use crate::error::{
        Error,
        Result
    };

fn create_userns() -> Result<()> {
    const UNSHARE_FLAGS: CloneFlags = CloneFlags::CLONE_NEWUSER | 
            CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID;
    if let Err(e) = unshare(UNSHARE_FLAGS) {
        log::error!("Failed to unshare user, mount, pid: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}

fn wait_root() -> Result<()> {
    const WAIT_INTERVAL: Duration = Duration::from_millis(10);
    for _ in 0..1000 {
        if getuid().is_root() && getgid().as_raw() == 0 {
            return Ok(())
        }
        sleep(WAIT_INTERVAL)
    }
    log::error!("We're not mapped to root, uid: {}, gid: {}",
                    getuid(), getgid());
    Err(Error::BrokenEnvironment)
}

pub(crate) fn main() -> Result<()>
{
    create_userns()?;
    wait_root()?;
    // let args = Args::parse_from(args);
    // let mut command = std::process::Command::new("/bin/bash");
    // if ! args.command.is_empty() {
    //     command.arg("-c")
    //         .arg(args.command);
    // };
    // command
    //     .spawn()
    //     .unwrap()
    //     .wait()
    //     .unwrap();
    Err(Error::ImpossibleLogic)
}