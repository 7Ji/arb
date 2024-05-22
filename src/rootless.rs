mod action;
mod broker;
mod arg0;
mod id;
mod idmap;
mod init;
mod root;
mod unshare;
use std::{ffi::{OsStr, OsString}, io::Write, iter::empty, path::{Path, PathBuf}, process::Child};
use nix::{libc::pid_t, unistd::getpid};
use pkgbuild::Architecture;
use crate::{child::{get_child_in_out, get_child_out, read_from_child, wait_child, write_to_child}, logfile::LogFile, pacman::try_get_install_pkgs_payload, pkgbuild::Pkgbuilds, Error, Result};
use self::{action::start_action, idmap::IdMaps};

pub(crate) use self::id::set_uid_gid;
pub(crate) use self::init::{InitCommand, InitPayload};
pub(crate) use self::broker::{BrokerCommand, BrokerPayload};
pub(crate) use self::root::{Root, chroot_checked};
pub(crate) use self::unshare::{
    try_unshare_user_and_wait,
    try_unshare_user_mount_and_wait,
    try_unshare_user_mount_pid_and_wait
};

pub(crate) struct RootlessHandler {
    idmaps: IdMaps,
    exe: PathBuf,
}

impl RootlessHandler {
    pub(crate) fn try_new() -> Result<Self> {
        id::ensure_no_root()?;
        IdMaps::ensure_not_mapped()?;
        let handler = Self { 
            idmaps: IdMaps::try_new()?, 
            exe: arg0::try_get_arg0()?
        };
        handler.run_action_no_arg_no_payload("map-assert")?;
        Ok(handler)
    }

    pub(crate) fn set_pid(&self, pid: pid_t) -> Result<()> {
        self.idmaps.set_pid(pid)
    }

    pub(crate) fn set_child(&self, child: &Child) -> Result<()> {
        self.idmaps.set_child(child)
    }

    fn map_child(&self, child: &mut Child) -> Result<()> {
        let r = 
            unshare::try_wait_as_parent(child)
                .and(self.set_child(child));
        if let Err(e) = &r {
            log::error!("Failed to map child {}: {}", child.id(), e)
        }
        r
    }

    fn map_and_wait_child(&self, child: &mut Child) -> Result<()> {
        self.map_child(child)?;
        wait_child(child)
    }

    pub(crate) fn run_action<S1, I, S2, B>(
        &self, applet: S1, args: I, payload: Option<B>
    ) -> Result<()> 
    where
        S1: AsRef<OsStr>,
        I: IntoIterator<Item = S2>,
        S2: AsRef<OsStr>,
        B: AsRef<[u8]>
    {
        let mut child = start_action(Some(&self.exe),
            applet, args, payload.is_some(), false)?;
        if let Some(payload) = payload {
            write_to_child(&mut child, payload)?
        }
        self.map_child(&mut child)?;
        wait_child(&mut child)
    }

    pub(crate) fn run_action_no_arg_no_payload<S>(&self, applet: S) 
        -> Result<()> 
    where
        S: AsRef<OsStr>,
    {
        self.run_action::<_, _, &str, &[u8]>(
            applet, empty(), None)
    }

    pub(crate) fn run_action_no_arg<S, B>(
        &self, applet: S, payload: Option<B>
    ) -> Result<()> 
    where
        S: AsRef<OsStr>,
        B: AsRef<[u8]>
    {
        self.run_action::<_, _, &str, _>(
            applet, empty(), payload)
    }

    pub(crate) fn run_action_no_payload<S1, I, S2>(
        &self, applet: S1, args: I
    ) -> Result<()> 
    where
        S1: AsRef<OsStr>,
        I: IntoIterator<Item = S2>,
        S2: AsRef<OsStr>
    {
        self.run_action::<_, _, _, &[u8]>(
            applet, args, None)
    }

    pub(crate) fn run_broker (&self, payload: &BrokerPayload) -> Result<()> {
        self.run_action_no_arg(
            "broker", 
            Some(payload.try_into_bytes()?))
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

    pub(crate) fn install_pkgs_to_root<I, S>(
        &self, root: &Root, pkgs: I, refresh: bool
    ) 
        -> Result<()> 
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>
    {
        let payload = try_get_install_pkgs_payload(
            root.get_path(), pkgs, refresh)?;
        self.run_broker(&payload)
    }

    pub(crate) fn bootstrap_root<I, S>(
        &self, root: &Root, pkgs: I, refresh: bool
    ) 
        -> Result<()> 
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>
    {
        let path_root = root.get_path();
        log::info!("Bootstrapping root at '{}'", path_root.display());
        let mut payload = try_get_install_pkgs_payload(
            root.get_path(), pkgs, refresh)?;
        payload.add_init_command(InitCommand::Chroot { 
            path: root.get_path().into() });
        payload.add_init_command_run_program(
            LogFile::try_new("bootstrap",
                "localedef-en_GB.UTF-8")?,
            "localedef", 
            ["-i", "en_GB", "-c", "-f", "UTF-8", "-A", 
                    "/usr/share/locale/locale.alias",  "en_GB.UTF-8"]);
        payload.add_init_command_run_program(
            LogFile::try_new("bootstrap", 
                "useradd-arb")?,
            "useradd",
            ["-u", "1000", "-m", "arb"]);
        payload.add_init_command_run_program(
            LogFile::try_new("pacman-key", 
                "init")?,
            "pacman-key",
            ["--init"]);
        payload.add_init_command_run_program(
            LogFile::try_new("pacman-key", 
                "populate")?,
            "pacman-key",
            ["--populate"]);
        self.run_broker(&payload)?;
        log::info!("Bootstrapped root at '{}'", path_root.display());
        Ok(())
    }

    fn start_broker(&self, pipe_out: bool) -> Result<Child> {
        start_action::<_, _, _, &str>(
            Some(&self.exe), "broker", empty(), 
            true, pipe_out)
    }

    fn map_and_write_to_child<B: AsRef<[u8]>> (
        &self, child: &mut Child, payload: B
    ) -> Result<()> {
        self.map_child(child)?;
        write_to_child(child, payload)
    }

    pub(crate) fn complete_pkgbuilds_in_root(
        &self, root: &Root, pkgbuilds: &mut Pkgbuilds
    ) -> Result<()> 
    {
        let payload = 
            pkgbuilds.get_reader_payload(root.get_path())
                .try_into_bytes()?;
        let mut child = self.start_broker(true)?;
        self.map_and_write_to_child(&mut child, &payload)?;
        pkgbuilds.complete_from_reader(get_child_out(&mut child)?)?;
        wait_child(&mut child)
    }

    pub(crate) fn dump_arch_in_root(&self, root: &Root) -> Result<Architecture>{
        let mut payload = root.new_broker_payload();
        payload.add_init_command_run_program("", "bash", 
            ["-c", "source /etc/makepkg.conf; echo $CARCH"]);
        let payload_bytes = payload.try_into_bytes()?;
        let mut child = self.start_broker(true)?;
        self.map_and_write_to_child(&mut child, &payload_bytes)?;
        let output = read_from_child(&mut child)?;
        wait_child(&mut child)?;
        Ok(String::from_utf8_lossy(&output).trim().into())
    }
}

/// Action: unshare all namespaces, wait to confirm mapping is OK
pub(crate) fn action_map_assert() -> Result<()> {
    if let Err(e) = unshare::try_unshare_user_mount_pid_and_wait() {
        log::error!("Mapping assertion failure");
        Err(e)
    } else {
        log::debug!("Mapping assertion success, rootless is functional, I am: \
            {}", id::ResUidGid::new()?);
        Ok(())
    }
}

/// Action: unshare all namespaces, then start init
pub(crate) fn action_broker() -> Result<()> 
{
    unshare::try_unshare_user_mount_pid_and_wait()?;
    BrokerPayload::try_read()?.work()
}

/// Action: psuedo init implementation to run external programs
pub(crate) fn action_init() -> Result<()> {
    // We shall be the first and only process spawned by parent, and being PID1
    let pid = getpid();
    if pid.as_raw() != 1 {
        log::error!("We're not PID 1 but {}", pid);
        return Err(Error::MappingFailure)
    }
    nix::sys::prctl::set_child_subreaper(true)?;
    // Spawn the child we needed
    InitPayload::try_read()?.work()
}
