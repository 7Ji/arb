mod id;
mod idmap;
mod root;
mod unshare;
use std::{ffi::OsStr, fs::read_link, os::unix::process::CommandExt, path::PathBuf, process::{Child, Command}};
use nix::{libc::pid_t, unistd::Pid};

use crate::{Error, Result};

use self::idmap::IdMaps;

pub(crate) struct Handler {
    idmaps: IdMaps,
    exe: PathBuf,
}

impl Handler {
    pub(crate) fn new() -> Result<Self> {
        id::ensure_no_root()?;
        IdMaps::ensure_not_mapped()?;
        let idmaps = IdMaps::new()?;
        let exe = match read_link("/proc/self/exe") {
            Ok(exe) => exe,
            Err(e) => {
                log::error!("Failed to read link '/proc/self/exe' to get \
                    actual arg0: {}", e);
                return Err(e.into())
            },
        };
        let handler = Self { idmaps, exe };
        handler.run_applet_noarg("map_assert")?;
        Ok(handler)
    }

    pub(crate) fn set_pid(&self, pid: pid_t) -> Result<()> {
        self.idmaps.set_pid(pid)
    }

    pub(crate) fn set_child(&self, child: &Child) -> Result<()> {
        self.idmaps.set_child(child)
    }

    pub(crate) fn run_applet<I, S1, S2>(&self, applet: S1, args: I) -> Result<()> 
    where
        I: IntoIterator<Item = S2>,
        S1: AsRef<OsStr>,
        S2: AsRef<OsStr>,
    {
        let mut child = match Command::new(&self.exe)
            .arg0(&applet)
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
            unshare::wait_as_parent(child.id() as pid_t)
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

    pub(crate) fn run_applet_noarg<S>(&self, applet: S) -> Result<()> 
    where
        S: AsRef<OsStr>,
    {
        self.run_applet::<_, _, S>(applet, [])
    }
}

// pub(crate) fn confirm_nonroot() {

pub(crate) fn map_assert_applet() -> Result<()> {
    if let Err(e) = unshare::all_and_wait() {
        log::error!("Mapping assertion failure");
        Err(e)
    } else {
        Ok(())
    }
}