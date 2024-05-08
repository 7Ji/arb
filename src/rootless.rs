mod arg0;
mod id;
mod idmap;
mod init;
mod root;
pub(crate) mod unshare;
use std::{ffi::OsStr, fs::read_link, path::{Path, PathBuf}, process::{Child, Command}};
use nix::{libc::pid_t, unistd::{getpid, Pid}};

use crate::{child::wait_child, mount::mount_proc, pacman::{install_pkgs, PacmanConfig}, Error, Result};
use self::idmap::IdMaps;
pub(crate) use self::root::Root;

pub(crate) struct RootlessHandler {
    idmaps: IdMaps,
    exe: PathBuf,
}

pub(crate) fn run_action_stateless<I, S1, S2>(
    applet: S1, args: I, noparse: bool
) -> Result<()> 
where
    I: IntoIterator<Item = S2>,
    S1: AsRef<OsStr>,
    S2: AsRef<OsStr>,
{
    let mut command = Command::new(&arg0::get_arg0());
    command.arg(&applet);
    if noparse {
        command.arg("--");
    }
    command.args(args);
    let mut child = match command .spawn() {
        Ok(child) => child,
        Err(e) => {
            log::error!("Failed to run applet '{}'", 
                        applet.as_ref().to_string_lossy());
            return Err(e.into())
        },
    };
    wait_child(&mut child)
}

impl RootlessHandler {
    pub(crate) fn try_new() -> Result<Self> {
        id::ensure_no_root()?;
        IdMaps::ensure_not_mapped()?;
        let handler = Self { 
            idmaps: IdMaps::try_new()?, 
            exe: arg0::try_get_arg0()?
        };
        handler.run_action_noarg("map-assert", false)?;
        Ok(handler)
    }

    pub(crate) fn set_pid(&self, pid: pid_t) -> Result<()> {
        self.idmaps.set_pid(pid)
    }

    pub(crate) fn set_child(&self, child: &Child) -> Result<()> {
        self.idmaps.set_child(child)
    }

    fn map_and_wait_child(&self, child: &mut Child) -> Result<()> {
        let r = 
            unshare::try_wait_as_parent(child)
                .and(self.set_child(child));
        if let Err(e) = &r {
            log::error!("Failed to map child {}: {}", child.id(), e)
        }
        wait_child(child)
    }

    pub(crate) fn run_external<I, S1, S2>(&self, program: S1, args: I) 
        -> Result<()> 
    where
        I: IntoIterator<Item = S2>,
        S1: AsRef<OsStr>,
        S2: AsRef<OsStr>,
    {
        let mut child = match Command::new(&self.exe)
            .arg("broker")
            .arg("--")
            .arg(&program)
            .args(args)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to run broker to run program '{}'", 
                            program.as_ref().to_string_lossy());
                return Err(e.into())
            },
        };
        self.map_and_wait_child(&mut child)
    }

    pub(crate) fn run_action<I, S1, S2>(
        &self, applet: S1, args: I, noparse: bool
    ) -> Result<()> 
    where
        I: IntoIterator<Item = S2>,
        S1: AsRef<OsStr>,
        S2: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.exe);
        command.arg(&applet);
        if noparse {
            command.arg("--");
        }
        command.args(args);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to run applet '{}'", 
                            applet.as_ref().to_string_lossy());
                return Err(e.into())
            },
        };
        self.map_and_wait_child(&mut child)
    }

    pub(crate) fn run_action_noarg<S>(&self, applet: S, noparse: bool) 
        -> Result<()> 
    where
        S: AsRef<OsStr>,
    {
        self.run_action::<_, _, S>(applet, [], noparse)
    }

    pub(crate) fn new_root<P: AsRef<Path>>(&self, path: P, temporary: bool) 
    -> Root 
    {
        let destroy_with_exe = if temporary { 
            log::info!("Creating temporary root at '{}'", path.as_ref().display());
            Some(&self.exe) 
        } else { 
            log::info!("Creating root at '{}'", path.as_ref().display());
            None 
        };
        Root::new(path, &self.idmaps, destroy_with_exe)
    }

    pub(crate) fn install_pkgs_to_root<S>(&self, root: &Root, pkgs: &Vec<S>) 
        -> Result<()> 
    where
        S: AsRef<str>
    {
        install_pkgs(&root.get_path_pacman_conf(), pkgs, self)
    }
}

/// Action: unshare all namespaces, wait to confirm mapping is OK
pub(crate) fn action_map_assert() -> Result<()> {
    if let Err(e) = unshare::try_unshare_user_mount_pid_and_wait() {
        log::error!("Mapping assertion failure");
        Err(e)
    } else {
        log::info!("Mapping assertion success, rootless is functional, I am: \
            {}", id::ResUidGid::new()?);
        Ok(())
    }
}

/// Action: unshare all namespaces, then start init
pub(crate) fn action_broker<I, S>(args: I) -> Result<()> 
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    unshare::try_unshare_user_mount_pid_and_wait()?;
    run_action_stateless("init", args, true)
}

/// Action: psuedo init implementation to run external programs
pub(crate) fn action_init<P, I, S>(program: P, args: I) -> Result<()>
where
    P: AsRef<Path>,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    // We shall be the first and only process spawned by parent, and being PID1
    let pid = getpid();
    if pid.as_raw() != 1 {
        log::error!("We're not PID 1 but {}", pid);
        return Err(Error::MappingFailure)
    }
    mount_proc("/proc")?;
    nix::sys::prctl::set_child_subreaper(true)?;
    // Spawn the child we needed
    let child = match Command::new(program.as_ref())
        .args(args)
        .spawn() 
    {
        Ok(child) => child,
        Err(e) => {
            log::error!("Failed to spawn child '{}'", 
                        program.as_ref().display());
            return Err(e.into())
        },
    };
    init::reaper(child)
}
