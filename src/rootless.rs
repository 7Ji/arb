mod id;
mod idmap;
mod root;
mod unshare;
use std::{ffi::OsStr, fs::read_link, path::PathBuf, process::{Child, Command}};
use nix::{libc::pid_t, unistd::Pid};

use crate::{Error, Result};
use self::idmap::IdMaps;
pub(crate) use self::unshare::all_and_try_wait as unshare_all_and_try_wait;

pub(crate) struct RootlessHandler {
    idmaps: IdMaps,
    exe: PathBuf,
}

impl RootlessHandler {
    pub(crate) fn try_new() -> Result<Self> {
        id::ensure_no_root()?;
        IdMaps::ensure_not_mapped()?;
        let idmaps = IdMaps::try_new()?;
        let exe = match read_link("/proc/self/exe") {
            Ok(exe) => exe,
            Err(e) => {
                log::error!("Failed to read link '/proc/self/exe' to get \
                    actual arg0: {}", e);
                return Err(e.into())
            },
        };
        let handler = Self { idmaps, exe };
        handler.run_action_noarg("map-assert")?;
        Ok(handler)
    }

    pub(crate) fn set_pid(&self, pid: pid_t) -> Result<()> {
        self.idmaps.set_pid(pid)
    }

    pub(crate) fn set_child(&self, child: &Child) -> Result<()> {
        self.idmaps.set_child(child)
    }

    pub(crate) fn run_action<I, S1, S2>(&self, applet: S1, args: I) -> Result<()> 
    where
        I: IntoIterator<Item = S2>,
        S1: AsRef<OsStr>,
        S2: AsRef<OsStr>,
    {
        let mut child = match Command::new(&self.exe)
            .arg(&applet)
            .args(args)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to run applet '{}'", 
                            applet.as_ref().to_string_lossy());
                return Err(e.into())
            },
        };
        let r = 
            unshare::try_wait_as_parent(&mut child)
                .and(self.set_child(&child));
        if let Err(e) = &r {
            log::error!("Failed to map child {}: {}", child.id(), e)
        }
        match child.wait() {
            Ok(status) => if status.success() {
                r
            } else {
                log::error!("Child {} bad return {}", child.id(), status);
                Err(Error::BadChild { 
                    pid: Some(Pid::from_raw(child.id() as pid_t)), 
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

    pub(crate) fn run_action_noarg<S>(&self, applet: S) -> Result<()> 
    where
        S: AsRef<OsStr>,
    {
        self.run_action::<_, _, S>(applet, [])
    }
}

pub(crate) fn action_map_assert() -> Result<()> {
    if let Err(e) = unshare_all_and_try_wait() {
        log::error!("Mapping assertion failure");
        Err(e)
    } else {
        log::info!("Mapping assertion success, rootless is functional");
        Ok(())
    }
}