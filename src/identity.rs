// Todo: drop all of this, use user namespaces, so we can operate fully without
// jumping back and forth between root and normal user

use std::{
        ffi::OsString,
        os::unix::{
            process::CommandExt,
            prelude::OsStringExt,
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

use nix::{
        errno::Errno,
        unistd::{
            getgid,
            getuid,
            Gid,
            Uid,
        }
    };

use crate::child::ForkedChild;

#[derive(Clone)]
struct Environment {
    shell: OsString,
    cwd: OsString,
    home: OsString,
    lang: OsString,
    user: OsString,
    path: OsString
}

fn get_pw_entry_from_uid(uid: libc::uid_t) -> Result<libc::passwd, ()> {
    let pw_entry = unsafe { libc::getpwuid(uid) };
    if pw_entry.is_null() {
        log::error!("getpwuid() call failed: {}", 
            std::io::Error::last_os_error());
        return Err(())
    }
    Ok(unsafe { pw_entry.read() })
}

fn get_something_raw_from_uid<F>(uid: libc::uid_t, f: F) -> Result<Vec<u8>, ()> 
where
    F: FnOnce(&libc::passwd) -> *mut libc::c_char
{
    let pw_entry = get_pw_entry_from_uid(uid)?;
    let attribute = f(&pw_entry);
    let len = unsafe { libc::strnlen(attribute, 0x1000) };
    let raw = unsafe {
        std::slice::from_raw_parts(attribute as *const u8, len) };
    Ok(raw.to_vec())
}

fn _get_home_raw_from_uid(uid: libc::uid_t) -> Result<Vec<u8>, ()> {
    get_something_raw_from_uid(uid, |passwd|passwd.pw_dir)
}

fn get_name_raw_from_uid(uid: libc::uid_t) -> Result<Vec<u8>, ()> {
    get_something_raw_from_uid(uid, |passwd|passwd.pw_name)
}

fn get_home_and_name_raw_from_uid(uid: libc::uid_t) 
    -> Result<(Vec<u8>, Vec<u8>), ()>
{
    let pw_entry = get_pw_entry_from_uid(uid)?;
    let pw_dir = pw_entry.pw_dir;
    let pw_name = pw_entry.pw_name;
    let len_dir = unsafe { libc::strnlen(pw_dir, 0x1000) };
    let len_name = unsafe { libc::strnlen(pw_name, 0x1000) };
    let raw_home = unsafe {
        std::slice::from_raw_parts(pw_dir as *const u8, len_dir) };
    let raw_name = unsafe {
        std::slice::from_raw_parts(pw_name as *const u8, len_name) };
    Ok((raw_home.to_vec(), raw_name.to_vec()))
}


impl Environment {
    fn init(uid: Uid) -> Result<Self, ()> {
        let (home_raw, name_raw) 
            = get_home_and_name_raw_from_uid(uid.as_raw())?;
        let cwd = std::env::current_dir().or_else(|e|{
            log::error!("Failed to get current dir: {}", e);
            Err(())
        })?.as_os_str().to_os_string();
        let path = std::env::var_os("PATH").ok_or_else(||{
            log::error!("Failed to get PATH from env");
        })?;
        Ok(Self {
            shell: OsString::from("/bin/bash"),
            cwd,
            home: OsString::from_vec(home_raw),
            lang: OsString::from("en_US.UTF-8"),
            user: OsString::from_vec(name_raw),
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
    cwd_no_root: PathBuf,
    home_path: PathBuf,
    _home_string: String
}

pub(crate) trait Identity {
    fn uid(&self) -> Uid;
    fn gid(&self) -> Gid;
    fn name(&self) -> &str;
    fn is_root(&self) -> bool {
        self.uid().is_root()
    }

    fn sete_raw(uid: Uid, gid: Gid) 
        -> Result<(), Errno>
    {
        nix::unistd::setegid(gid)?;
        nix::unistd::seteuid(uid)
    }

    fn set_raw(uid: Uid, gid: Gid) 
        -> Result<(), Errno> 
    {
        nix::unistd::setgid(gid)?;
        nix::unistd::setuid(uid)
    }

    fn sete(&self) -> Result<(), std::io::Error> {
        Self::sete_raw(self.uid(), self.gid()).map_err(|e|e.into())
    }

    fn sete_root() -> Result<(), std::io::Error> {
        Self::sete_raw(
            Uid::from_raw(0), Gid::from_raw(0))
                .map_err(|e|e.into())
    }

    /// Return to root
    fn set_root_command(command: &mut Command) -> &mut Command {
        unsafe {
            command.pre_exec(|| Self::sete_root());
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
    ) -> Result<(), ()>
    {
        let r = Self::set_chroot_command(command, root)
            .output()
            .or_else(|e|{
                log::error!("Failed to spawn chroot command {:?}: {}", 
                    command, e);
                Err(())
            })?
            .status
            .code()
            .ok_or_else(||{
                log::error!("Failed to get exit code for chroot command {:?}",
                            command);
                ()
            })?;
        if r == 0 {
            Ok(())
        } else {
            log::error!("Bad return {} from chroot command {:?}", r, command);
            Err(())
        }
    }
    
    fn fork_and_run_child<F: FnOnce() -> Result<(), ()>,>(f: F)  
        -> Result<ForkedChild, ()>
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
                Err(())
            },
        }
    }

    fn fork_and_run<F: FnOnce() -> Result<(), ()>,>(f: F)  -> Result<(), ()>
    {
        Self::fork_and_run_child(f)?.wait()
    }

    /// Run a block as root in a forked child
    fn _as_root_with_chroot<F, P>(f: F, root: P) 
        -> Result<(), ()>
    where 
        F: FnOnce() -> Result<(), ()>,
        P: AsRef<Path>
    {
        Self::fork_and_run(||{
            if let Err(e) = chroot(root.as_ref()) {
                log::error!("Child: Failed to chroot to '{}': {}", 
                    root.as_ref().display(), e);
                return Err(())
            }
            if Self::sete_root().is_err() {
                log::error!("Child: Failed to seteuid back to root");
                return Err(())
            }
            f()
        })
    }

    fn as_root<F: FnOnce() -> Result<(), ()>>(f: F) -> Result<(), ()>
    {
        Self::fork_and_run(||{
            if Self::sete_root().is_err() {
                log::error!("Child: Failed to seteuid back to root");
                return Err(())
            }
            f()
        })
    }

    fn as_root_child<F: FnOnce() -> Result<(), ()>>(f: F) 
        -> Result<ForkedChild, ()>
    {
        Self::fork_and_run_child(||{
            if Self::sete_root().is_err() {
                log::error!("Child: Failed to seteuid back to root");
                return Err(())
            }
            f()
        })
    }

    fn _with_chroot<F, P>(f: F, root: P) 
        -> Result<(), ()>
    where 
        F: FnOnce() -> Result<(), ()>,
        P: AsRef<Path>
    {
        Self::fork_and_run(||{
            if let Err(e) = chroot(root.as_ref()) {
                log::error!("Child: Failed to chroot to '{}': {}", 
                    root.as_ref().display(), e);
                return Err(())
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
    pub(crate) fn new() -> Result<Self, ()> {
        let uid = getuid();
        let name_raw = get_name_raw_from_uid(uid.as_raw())?;
        Ok(Self {
            uid,
            gid: getgid(),
            name: String::from_utf8_lossy(&name_raw).to_string(),
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
    pub(crate) fn cwd_no_root(&self) -> &Path {
        &self.cwd_no_root
    }
    // pub(crate) fn cwd_absolute(&self) -> &Path {
    //     &self.cwd_absolute
    // }
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn home_path(&self) -> &Path {
        &self.home_path
    }
    pub(crate) fn _home_str(&self) -> &str {
        &self._home_string
    }

    fn new(uid: Uid, gid: Gid) -> Result<Self, ()> {
        let env = Environment::init(uid).or_else(|_|{
            log::info!("Failed to get env for actual user");
            Err(())
        })?;     
        let cwd = PathBuf::from(&env.cwd);
        let cwd_no_root = cwd.strip_prefix("/").or_else(
        |e|{
            log::error!("Failed to strip leading / from cwd: {}", e);
            Err(())
        })?.to_path_buf();
        let name = env.user.to_string_lossy().to_string();
        let home_path = PathBuf::from(&env.home);
        let _home_string = env.home.to_string_lossy().to_string();
        Ok(Self {
            uid,
            gid,
            name,
            env,
            cwd,
            cwd_no_root,
            home_path,
            _home_string
        })

    }

    fn new_from_id_pair(id_pair: &str) -> Result<Self, ()> {
        let components: Vec<&str> = id_pair
            .splitn(2, ':')
            .collect();
        if components.len() != 2 {
            log::error!("ID pair '{}' syntax incorrect", id_pair);
            return Err(())
        }
        let components: [&str; 2] = match components.try_into() {
            Ok(components) => components,
            Err(_) => {
                log::error!("Failed to convert identity components to array");
                return Err(())
            },
        };
        if let Ok(uid) = components[0].parse() {
        if let Ok(gid) = components[1].parse() {
            return Self::new(Uid::from_raw(uid), Gid::from_raw(gid));
        }
        }
        log::error!("Can not parse ID pair '{}'", id_pair);
        Err(())
    }

    fn new_from_sudo() -> Result<Self, ()> {
        if let Some(sudo_uid) = std::env::var_os("SUDO_UID") {
        if let Some(sudo_gid) = std::env::var_os("SUDO_GID") {
        if let Ok(uid) = sudo_uid.to_string_lossy().parse(){
        if let Ok(gid) = sudo_gid.to_string_lossy().parse(){
            return Self::new(
                Uid::from_raw(uid), Gid::from_raw(gid))
        }}}}
        Err(())
    }

    pub(crate) fn drop(&self) -> Result<&Self, ()> {
        let current = IdentityCurrent::new();
        if ! current?.is_root() {
            log::error!("Current user is not root, please run builder with sudo");
            return Err(())
        }
        if self.is_root() {
            log::error!("Actual user is root, please run builder with sudo");
            return Err(())
        }
        match self.sete() {
            Ok(_) => {
                log::info!("Dropped from root to {}", self);
                Ok(self)
            },
            Err(_) => {
                log::error!("Failed to drop from root to {}", self);
                Err(())
            },
        }
    }

    pub(crate) fn new_and_drop(id_pair: Option<&str>) -> Result<Self, ()> {
        let identity = match match id_pair {
            Some(id_pair) => 
                Self::new_from_id_pair(id_pair),
            None => Self::new_from_sudo(),
        } {
            Ok(identity) => identity,
            Err(_) => {
                log::error!("Failed to get actual identity, did you start the \
                    builder with sudo as a non-root user?");
                return Err(())
            },
        };
        match identity.drop() {
            Ok(_) => Ok(identity),
            Err(_) => Err(()),
        }
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