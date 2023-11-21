// Todo: drop all of this, use user namespaces, so we can operate fully without
// jumping back and forth between root and normal user

use std::{
        ffi::OsString,
        os::unix::{
            process::CommandExt,
            fs::chroot
        },
        process::{
            Command,
            exit
        },
        path::{
            PathBuf,
            Path
        }, fmt::Display,
    };

use nix::unistd::{
        getgid,
        getuid,
        Gid,
        Uid,
    };

use pwd::Passwd;

use crate::{
        child::{
            ForkedChild,
            output_and_check,
        },
        error::{
            Error,
            Result,
        },
    };

#[derive(Clone)]
struct Environment {
    shell: OsString,
    cwd: OsString,
    home: OsString,
    lang: OsString,
    user: OsString,
    path: OsString
}

fn passwd_from_uid_checked(uid: Uid) -> Result<Passwd> {
    if let Some(pwd) = pwd::Passwd::from_uid(uid.into()) {
        Ok(pwd)
    } else {
        Err(Error::BrokenEnvironment)
    }
}

impl Environment {
    fn init(uid: Uid) -> Result<Self> {
        let passwd = passwd_from_uid_checked(uid)?;;
        let cwd = std::env::current_dir().map_err(Error::from)?
            .as_os_str().to_os_string();
        let path = std::env::var_os("PATH").ok_or_else(||{
            log::error!("Failed to get PATH from env");
            Error::BrokenEnvironment
        })?;
        Ok(Self {
            shell: OsString::from("/bin/bash"),
            cwd,
            home: passwd.dir.into(),
            lang: OsString::from("en_US.UTF-8"),
            user: passwd.name.into(),
            path,
        })

    }
    fn set_command<'a> (&self, command: &'a mut Command) -> &'a mut Command {
        command
            .env_remove("SUDO_UID")
            .env_remove("SUDO_GID")
            .env_remove("SUDO_USER")
            .env("SHELL", &self.shell)
            .env("PWD", &self.cwd)
            .env("LOGNAME", &self.user)
            .env("HOME", &self.home)
            .env("LANG", &self.lang)
            .env("USER", &self.user)
            .env("PATH", &self.path)
    }
}

#[derive(Clone)]
pub(crate) struct IdentityCurrent {
    uid: Uid,
    gid: Gid,
    name: String,
}

#[derive(Clone)]
pub(crate) struct IdentityActual {
    uid: Uid,
    gid: Gid,
    name: String,
    env: Environment,
    cwd: PathBuf,
    home: PathBuf,
}

pub(crate) trait Identity {
    fn uid(&self) -> Uid;
    fn gid(&self) -> Gid;
    fn name(&self) -> &str;
    fn is_root(&self) -> bool {
        self.uid().is_root()
    }

    fn sete_raw(uid: Uid, gid: Gid)
        -> Result<()>
    {
        nix::unistd::setegid(gid).map_err(Error::from)?;
        nix::unistd::seteuid(uid).map_err(Error::from)
    }

    fn set_raw(uid: Uid, gid: Gid)
        -> Result<()>
    {
        nix::unistd::setgid(gid).map_err(Error::from)?;
        nix::unistd::setuid(uid).map_err(Error::from)
    }

    fn sete(&self) -> Result<()> {
        Self::sete_raw(self.uid(), self.gid()).map_err(|e|e.into())
    }

    fn sete_root() -> Result<()> {
        Self::sete_raw(
            Uid::from_raw(0), Gid::from_raw(0))
                .map_err(|e|e.into())
    }

    /// Return to root
    fn set_root_command(command: &mut Command) -> &mut Command {
        unsafe {
            command.pre_exec(|| Self::sete_root().map_err(Error::into));
        }
        command
    }


    /// Chroot to a folder, as this uses chroot(), you need to return to root
    /// first
    fn set_chroot_command<P: AsRef<Path>>(
        command: &mut Command, root: P
    ) -> &mut Command
    {
        let root = root.as_ref().to_owned();
        unsafe {
            command.pre_exec(move || chroot(&root));
        }
        command
    }

    fn run_chroot_command<P: AsRef<Path>>(
        command: &mut Command, root: P
    ) -> Result<()>
    {
        output_and_check(Self::set_chroot_command(command, root),
                         "chrooted")
    }

    fn fork_and_run_child<F: FnOnce() -> Result<()>,>(f: F)
        -> Result<ForkedChild>
    {
        match unsafe{ nix::unistd::fork() } {
            Ok(result) => match result {
                nix::unistd::ForkResult::Parent { child } =>
                    Ok(ForkedChild { pid: child }),
                nix::unistd::ForkResult::Child =>
                    if f().is_err() {
                        exit(-1)
                    } else {
                        exit(0)
                    },
            },
            Err(e) => {
                log::error!("Failed to fork: {}", e);
                Err(e.into())
            },
        }
    }

    fn fork_and_run<F: FnOnce() -> Result<()>,>(f: F)  -> Result<()>
    {
        Self::fork_and_run_child(f)?.wait()
    }

    /// Run a block as root in a forked child
    fn _as_root_with_chroot<F, P>(f: F, root: P)
        -> Result<()>
    where
        F: FnOnce() -> Result<()>,
        P: AsRef<Path>
    {
        Self::fork_and_run(||{
            if let Err(e) = chroot(root.as_ref()) {
                log::error!("Child: Failed to chroot to '{}': {}",
                    root.as_ref().display(), e);
                return Err(e.into())
            }
            if let Err(e) = Self::sete_root() {
                log::error!("Child: Failed to seteuid back to root: {}", e);
                return Err(e.into())
            }
            f()
        })
    }

    fn as_root<F: FnOnce() -> Result<()>>(f: F) -> Result<()>
    {
        Self::fork_and_run(||{
            if let Err(e) = Self::sete_root() {
                log::error!("Child: Failed to seteuid back to root: {}", e);
                return Err(e.into())
            }
            f()
        })
    }

    fn as_root_child<F: FnOnce() -> Result<()>>(f: F)
        -> Result<ForkedChild>
    {
        Self::fork_and_run_child(||{
            if let Err(e) = Self::sete_root() {
                log::error!("Child: Failed to seteuid back to root: {}", e);
                return Err(e.into())
            }
            f()
        })
    }

    fn _with_chroot<F, P>(f: F, root: P)
        -> Result<()>
    where
        F: FnOnce() -> Result<()>,
        P: AsRef<Path>
    {
        Self::fork_and_run(||{
            if let Err(e) = chroot(root.as_ref()) {
                log::error!("Child: Failed to chroot to '{}': {}",
                    root.as_ref().display(), e);
                return Err(e.into())
            }
            f()
        })
    }
}

impl Identity for IdentityCurrent {
    fn uid(&self) -> Uid {
        self.uid
    }

    fn gid(&self) -> Gid {
        self.gid
    }

    fn name(&self) -> &str {
        &self.name
    }
}


impl Display for IdentityCurrent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "uid: {}, gid: {}, name: {}", self.uid, self.gid, self.name))
    }
}

impl IdentityCurrent {
    pub(crate) fn new() -> Result<Self> {
        let uid = getuid();
        let name = passwd_from_uid_checked(uid)?.name;
        Ok(Self {
            uid,
            gid: getgid(),
            name,
        })
    }
}

impl Identity for IdentityActual {
    fn uid(&self) -> Uid {
        self.uid
    }

    fn gid(&self) -> Gid {
        self.gid
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Display for IdentityActual {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "uid: {}, gid: {}, name: {}", self.uid, self.gid, self.name))
    }
}

impl IdentityActual {
    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }
    pub(crate) fn cwd_no_root(&self) -> Result<&Path> {
        match self.cwd.strip_prefix("/") {
            Ok(path) => Ok(path),
            Err(e) => {
                log::error!("CWD path '{}' does not start with /, strip error: \
                   {}", self.cwd.display(), e);
                Err(Error::BrokenEnvironment)
            },
        }
    }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }
    pub(crate) fn home_no_root(&self) -> Result<&Path> {
        match self.home.strip_prefix("/") {
            Ok(path) => Ok(path),
            Err(e) => {
                log::error!("Home path '{}' does not start with /, \
                    check you passwd config, strip error: {}", 
                    self.home.display(), e);
                Err(Error::BrokenEnvironment)
            },
        }
    }

    fn new(uid: Uid, gid: Gid) -> Result<Self> {
        match Environment::init(uid) {
            Ok(env) => {
                let cwd = PathBuf::from(&env.cwd);
                let home = PathBuf::from(&env.home);
                Ok(Self {
                    uid,
                    gid,
                    name: env.user.to_string_lossy().to_string(),
                    env,
                    cwd,
                    home, 
                })
            },
            Err(e) => {
                log::error!("Failed to get env for actual user: {}", e);
                return Err(e)
            },
        }
    }

    fn new_from_id_pair(id_pair: &str) -> Result<Self> {
        let components: Vec<&str> = id_pair
            .splitn(2, ':')
            .collect();
        if components.len() != 2 {
            log::error!("ID pair '{}' syntax incorrect", id_pair);
            return Err(Error::InvalidConfig)
        }
        let components: [&str; 2] = match components.try_into() {
            Ok(components) => components,
            Err(_) => {
                log::error!("Failed to convert identity components to array");
                return Err(Error::ImpossibleLogic)
            },
        };
        if let Ok(uid) = components[0].parse() {
        if let Ok(gid) = components[1].parse() {
            return Self::new(Uid::from_raw(uid), Gid::from_raw(gid));
        }
        }
        log::error!("Can not parse ID pair '{}'", id_pair);
        Err(Error::InvalidConfig)
    }

    fn new_from_sudo() -> Result<Self> {
        if let Some(sudo_uid) = std::env::var_os("SUDO_UID") {
        if let Some(sudo_gid) = std::env::var_os("SUDO_GID") {
        if let Ok(uid) = sudo_uid.to_string_lossy().parse(){
        if let Ok(gid) = sudo_gid.to_string_lossy().parse(){
            return Self::new(
                Uid::from_raw(uid), Gid::from_raw(gid))
        }}}}
        Err(Error::BrokenEnvironment)
    }

    pub(crate) fn drop(&self) -> Result<&Self> {
        if ! IdentityCurrent::new()?.is_root() {
            log::error!("Current user is not root, please run builder with sudo");
            return Err(Error::BrokenEnvironment)
        }
        if self.is_root() {
            log::error!("Actual user is root, please run builder with sudo");
            return Err(Error::BrokenEnvironment)
        }
        if let Err(e) = self.sete() {
            log::error!("Failed to drop from root to {}", self);
            Err(e.into())
        } else {
            log::info!("Dropped from root to {}", self);
            Ok(self)
        }
    }

    pub(crate) fn new_and_drop(id_pair: Option<&str>) -> Result<Self> {
        let identity = match match id_pair {
            Some(id_pair) =>
                Self::new_from_id_pair(id_pair),
            None => Self::new_from_sudo(),
        } {
            Ok(identity) => identity,
            Err(e) => {
                log::error!("Failed to get actual identity, did you start the \
                    builder with sudo as a non-root user?");
                return Err(e)
            },
        };
        identity.drop()?;
        Ok(identity)
    }

    /// Drop to the identity, as this uses setuid/setgid, you need to return
    /// to root first
    pub(crate) fn set_drop_command<'a>(&self, command: &'a mut Command)
        -> &'a mut Command
    {
        self.env.set_command(command);
        let uid = self.uid;
        let gid = self.gid;
        unsafe {
            command.pre_exec(move ||
                Self::set_raw(uid, gid).map_err(|e|e.into()));
        }
        command
    }

    /// Return to root then drop
    pub(crate) fn set_root_drop_command<'a>(&self, command: &'a mut Command)
        -> &'a mut Command
    {
        self.env.set_command(command);
        Self::set_root_command(command);
        self.set_drop_command(command)
    }

    /// Return to root, chroot to a folder, then drop
    pub(crate) fn set_root_chroot_drop_command<'a, 'b, P: AsRef<Path>>(
        &'a self, command: &'b mut Command, root: P
    ) -> &'b mut Command
    {
        self.env.set_command(command);
        Self::set_root_command(command);
        Self::set_chroot_command(command, root);
        self.set_drop_command(command)
        // command
    }
}