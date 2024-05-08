mod arg0;
mod id;
mod idmap;
mod init;
mod root;
pub(crate) mod unshare;
use std::{ffi::OsStr, iter::empty, path::{Path, PathBuf}, process::Child};
use nix::{libc::pid_t, unistd::getpid};
use rayon::iter::Empty;

use crate::{child::{command_new_no_stdin, wait_child}, mount::{mount_all, mount_all_except_proc, mount_proc}, pacman::{install_pkgs, sync_db, PacmanConfig}, Error, Result};
use self::idmap::IdMaps;
pub(crate) use self::root::Root;

pub(crate) struct RootlessHandler {
    idmaps: IdMaps,
    exe: PathBuf,
}

pub(crate) fn run_action_stateless_with_sub_args<S1, I1, S2, I2, S3>(
    applet: S1, main_args: I1, sub_args: Option<I2>
) -> Result<()> 
where
    S1: AsRef<OsStr>,
    I1: IntoIterator<Item = S2>,
    S2: AsRef<OsStr>,
    I2: IntoIterator<Item = S3>,
    S3: AsRef<OsStr>,
{
    let mut command = command_new_no_stdin(&arg0::get_arg0());
    command.arg(&applet).args(main_args);
    if let Some(args) = sub_args {
        command.arg("--").args(args);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            log::error!("Failed to run applet '{}'", 
                        applet.as_ref().to_string_lossy());
            return Err(e.into())
        },
    };
    wait_child(&mut child)
}

pub(crate) fn run_action_stateless<S1, I1, S2>(applet: S1, main_args: I1)
    -> Result<()> 
where
    S1: AsRef<OsStr>,
    I1: IntoIterator<Item = S2>,
    S2: AsRef<OsStr>
{
    let mut child = match 
        command_new_no_stdin(&arg0::get_arg0())
            .arg(&applet).args(main_args).spawn() 
    {
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
        handler.run_action_noarg("map-assert")?;
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

    pub(crate) fn run_external<I, S1, S2, S3>(
        &self, program: S1, root: S2, args: I
    ) -> Result<()> 
    where
        I: IntoIterator<Item = S3>,
        S1: AsRef<OsStr>,
        S2: AsRef<OsStr>,
        S3: AsRef<OsStr>,
    {
        let mut command = command_new_no_stdin(&self.exe);
        command.arg("broker");
        if ! root.as_ref().is_empty() {
            command.arg("--root").arg(root);
        }
        let mut child = match command
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

    pub(crate) fn run_action_with_sub_args<S1, I1, S2, I2, S3>(
        &self, applet: S1, main_args: I1, sub_args: Option<I2>
    ) -> Result<()> 
    where
        S1: AsRef<OsStr>,
        I1: IntoIterator<Item = S2>,
        S2: AsRef<OsStr>,
        I2: IntoIterator<Item = S3>,
        S3: AsRef<OsStr>,
    {
        let mut command = command_new_no_stdin(&self.exe);
        command.arg(&applet).args(main_args);
        if let Some(args) = sub_args {
            command.arg("--").args(args);
        }
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

    pub(crate) fn run_action<S1, I1, S2>(&self, applet: S1, main_args: I1) 
        -> Result<()> 
    where
        S1: AsRef<OsStr>,
        I1: IntoIterator<Item = S2>,
        S2: AsRef<OsStr>
    {
        let mut child = match 
            command_new_no_stdin(&self.exe).arg(&applet).args(main_args).spawn() 
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to run applet '{}'", 
                            applet.as_ref().to_string_lossy());
                return Err(e.into())
            },
        };
        self.map_and_wait_child(&mut child)
    }

    pub(crate) fn run_action_noarg<S>(&self, applet: S) 
        -> Result<()> 
    where
        S: AsRef<OsStr>,
    {
        self.run_action::<_, _, &str>(applet, empty())
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

    pub(crate) fn sync_db_for_root(&self, root: &Root) 
        -> Result<()> 
    {
        sync_db(&root.get_path_pacman_conf(), self)
    }

    pub(crate) fn install_pkgs_to_root<S>(&self, root: &Root, pkgs: &Vec<S>) 
        -> Result<()> 
    where
        S: AsRef<str>
    {
        install_pkgs(root.get_path(), pkgs, self)
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
pub(crate) fn action_broker(root: &str, args: &Vec<String>) -> Result<()> 
{
    unshare::try_unshare_user_mount_pid_and_wait()?;
    let mut args_prepend = Vec::new();
    if ! root.is_empty() {
        mount_all_except_proc(root)?;
        args_prepend.push("--proc".into());
        args_prepend.push(format!("{}/proc", root));
    }
    run_action_stateless_with_sub_args(
        "init", args_prepend, Some(args))
}

/// Action: psuedo init implementation to run external programs
pub(crate) fn action_init<P, I, S>(proc: &str, program: P, args: I) 
    -> Result<()>
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
    if ! proc.is_empty() {
        mount_proc(proc)?
    }
    nix::sys::prctl::set_child_subreaper(true)?;
    // Spawn the child we needed
    let child = match command_new_no_stdin(program.as_ref())
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
