use std::{fs::read_link, thread::sleep, time::Duration};

use nix::{libc::pid_t, sched::{unshare, CloneFlags}, unistd::{getpid, getresgid, getresuid}};
use crate::{Error, Result};

use super::id::ResUidGid;

const WAIT_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) fn wait_as_parent(child: pid_t) -> Result<()> {
    let ns_user_parent = match read_link("/proc/self/ns/user") {
        Ok(ns) => ns,
        Err(e) => {
            log::error!("Failed to read parent user ns link: {}", e);
            return Err(e.into())
        },
    };
    let link = format!("/proc/{}/ns/user", child);
    for _ in 0..1000 {
        let ns_user_child = match read_link(&link) {
            Ok(ns) => ns,
            Err(e) => {
                log::error!("Failed to read child user ns link: {}", e);
                return Err(e.into())
            },
        };
        if ns_user_child != ns_user_parent {
            return Ok(())
        }
        sleep(WAIT_INTERVAL)
    }
    log::error!("Child {} did not unshare user namespaces", child);
    Err(Error::MappingFailure)
}

fn wait_as_child() -> Result<()> {
    for i in 0..1000 {
        let res_uid_gid = ResUidGid::new()?;
        if res_uid_gid.is_root() {
            return Ok(())
        }
        if i == 999 {
            log::error!("Child {}: We were not mapped to root: {}", getpid(),
                        res_uid_gid);
            break
        }
        sleep(WAIT_INTERVAL)
    }
    Err(Error::MappingFailure)
}

pub(crate) fn all() -> Result<()> {
    if let Err(e) = unshare(CloneFlags::CLONE_NEWUSER | 
                    CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID) 
    {
        log::error!("Failed to unshare user, mount and pid from parent: {}", e);
        Err(e.into())
    } else {
        log::info!("Child {}: We've unshared namespaces from root, wait for \
            parent to map us to root...", getpid());
        Ok(())
    }
}

pub(crate) fn all_and_wait() -> Result<()> {
    all()?;
    wait_as_child()
}