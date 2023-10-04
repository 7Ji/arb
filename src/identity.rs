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
        str::FromStr,
        path::{
            PathBuf,
            Path
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

impl Environment {
    fn init(uid: libc::uid_t, name: &str) -> Option<Self> {
        let pw_dir = unsafe {
            let pw_dir = libc::getpwuid(uid).read().pw_dir;
            let len_dir = libc::strlen(pw_dir);
            std::slice::from_raw_parts(pw_dir as *const u8, len_dir)
        };
        Some(Self {
            shell: OsString::from("/bin/bash"),
            cwd: std::env::current_dir().ok()?.as_os_str().to_os_string(),
            home: OsString::from_vec(pw_dir.to_vec()),
            lang: OsString::from("en_US.UTF-8"),
            user: OsString::from_str(name).ok()?,
            path: std::env::var_os("PATH")?,
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
pub(crate) struct Identity {
    uid: u32,
    gid: u32,
    name: String,
    env: Option<Environment>,
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "uid: {}, gid: {}, name: {}", self.uid, self.gid, self.name))
    }
}

impl Identity {
    pub(crate) fn current() -> Self {
        Self {
            uid: unsafe {
                libc::getuid()
            },
            gid: unsafe {
                libc::getgid()
            },
            name: unsafe {
                let ptr = libc::getlogin();
                let len = libc::strlen(ptr);
                let slice = std::slice::from_raw_parts(
                    ptr as *mut u8, len);
                String::from_utf8_lossy(slice).into_owned()
            },
            env: None,
        }
    }

    pub(crate) fn actual() -> Self {
        if let Some(sudo_uid) = std::env::var_os("SUDO_UID") {
        if let Some(sudo_gid) = std::env::var_os("SUDO_GID") {
        if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
        if let Ok(uid) = sudo_uid.to_string_lossy().parse() {
        if let Ok(gid) = sudo_gid.to_string_lossy().parse() {
            let name = sudo_user.to_string_lossy();
            let env = 
                Environment::init(uid, &name)
                .expect("Failed to get env for acutal user");
            return Self {
                uid,
                gid,
                name: name.to_string(),
                env: Some(env),
            }
        }}}}}
        Self::current()
    }

    fn is_root(&self) -> bool {
        self.uid == 0 && self.gid == 0
    }

    fn _is_real_root(&self) -> bool {
        self.is_root() && self.name == "root"
    }

    fn _is_sudo_root() -> bool {
        Self::current().is_root() && !Self::actual().is_root()
    }

    fn current_and_actual() -> (Self, Self) {
        (Self::current(), Self::actual())
    }

    fn sete_raw(uid: libc::uid_t, gid: libc::gid_t) 
        -> Result<(), std::io::Error>
    {
        let r = unsafe { libc::setegid(gid) };
        if r != 0 {
            eprintln!("Failed to setegid to {}: return {}, errno {}", 
                gid, r, unsafe {*libc::__errno_location()});
            return Err(std::io::Error::last_os_error())
        }
        let r = unsafe { libc::seteuid(uid) };
        if r != 0 {
            eprintln!("Failed to seteuid to {}: return {}, errno {}", 
                uid, r, unsafe {*libc::__errno_location()});
            return Err(std::io::Error::last_os_error())
        }
        Ok(())
    }

    fn set_raw(uid: libc::uid_t, gid: libc::gid_t) 
        -> Result<(), std::io::Error> 
    {
        let r = unsafe { libc::setgid(gid) };
        if r != 0 {
            eprintln!("Failed to setgid to {}: return {}, errno {}", 
                gid, r, unsafe {*libc::__errno_location()});
            return Err(std::io::Error::last_os_error())
        }
        let r = unsafe { libc::setuid(uid) };
        if r != 0 {
            eprintln!("Failed to setuid to {}: return {}, errno {}", 
                uid, r, unsafe {*libc::__errno_location()});
            return Err(std::io::Error::last_os_error())
        }
        Ok(())
    }

    fn sete(&self) -> Result<(), std::io::Error> {
        Self::sete_raw(self.uid, self.gid)
    }

    fn sete_root() -> Result<(), std::io::Error> {
        Self::sete_raw(0, 0)
    }

    pub(crate) fn set_root_command(command: &mut Command) -> &mut Command {
        unsafe {
            command.pre_exec(|| Self::sete_root());
        }
        command
    }

    pub(crate) fn set_drop_command<'a>(&self, command: &'a mut Command) 
        -> &'a mut Command 
    {
        self.env.as_ref().expect("Env not parsed")
            .set_command(command);
        let uid = self.uid;
        let gid = self.gid;
        unsafe {
            command.pre_exec(move || Self::set_raw(uid, gid));
        }
        command
    }

    pub(crate) fn set_command<'a>(&self, command: &'a mut Command) 
        -> &'a mut Command 
    {
        self.env.as_ref().expect("Env not parsed")
            .set_command(command);
        Self::set_root_command(command);
        self.set_drop_command(command)
    }

    pub(crate) fn set_chroot_command<P: AsRef<Path>>(
        command: &mut Command, root: P
    ) -> &mut Command
    {
        let root = root.as_ref().to_owned();
        unsafe {
            command.pre_exec(move || chroot(&root));
        }
        command
    }

    pub(crate) fn set_chroot_drop_command<'a, 'b, P: AsRef<Path>>(
        &'a self, command: &'b mut Command, root: P
    ) -> &'b mut Command
    {
        self.env.as_ref().expect("Env not parsed")
            .set_command(command);
        Self::set_root_command(command);
        Self::set_chroot_command(command, root);
        self.set_drop_command(command)
        // command
    }

    pub(crate) fn run_chroot_command<P: AsRef<Path>>(
        command: &mut Command, root: P
    ) -> Result<(), ()>
    {
        let r = Identity::set_chroot_command(command, root)
            .spawn()
            .or_else(|e|{
                eprintln!("Failed to spawn chroot command {:?}: {}", 
                    command, e);
                Err(())
            })?
            .wait()
            .or_else(|e|{
                eprintln!("Failed to wait for chroot command {:?}: {}",
                     command, e);
                Err(())
            })?
            .code()
            .ok_or_else(||{
                eprintln!("Failed to get exit code for chroot command {:?}",
                            command);
                ()
            })?;
        if r == 0 {
            Ok(())
        } else {
            eprintln!("Bad return from chroot command {:?}", command);
            Err(())
        }
    }
    // pub(crate) fn chroot_command<P: AsRef<Path>>(
    //     command: &mut Command, root: P
    // ) -> Result<(), ()> 
    // {
    //     Self::with_chroot(|| {
    //         let child = match command.exec() {
    //             Ok(child) => ,
    //             Err(_) => todo!(),
    //         }
    //     }, root)
    // }

    fn fork_and_run<F: FnOnce() -> Result<(), ()>,>(f: F)  -> Result<(), ()>
    {
        let child = unsafe {
            libc::fork()
        };
        if child == 0 { // I am child
            if f().is_err() {
                exit(-1)
            } else {
                exit(0)
            }
        } else if child < 0 { // Error encountered
            eprintln!("Failed to fork: {}", std::io::Error::last_os_error());
            return Err(())
        }
        // I am parent
        let mut status: libc::c_int = 0;
        let waited_pid = unsafe {
            libc::waitpid(child, &mut status, 0)
        };
        if waited_pid <= 0 {
            eprintln!("Failed to wait for child: {}", 
                std::io::Error::last_os_error());
            return Err(())
        }
        if waited_pid != child {
            eprintln!("Waited child {} is not the child {} we forked", 
                        waited_pid, child);
            return Err(())
        }
        if status != 0 {
            eprintln!("Child process failed");
            return Err(())
        }
        Ok(())
        
    }

    pub(crate) fn get_actual_and_drop() -> Result<Self, ()> {
        let (current, actual) = Self::current_and_actual();
        if ! current.is_root() {
            eprintln!("Current user is not root, please run builder with sudo");
            return Err(())
        }
        if actual.is_root() {
            eprintln!("Actual user is root, please run builder with sudo");
            return Err(())
        }
        match actual.sete() {
            Ok(_) => {
                println!("Dropped from root to {}", actual);
                Ok(actual)
            },
            Err(_) => {
                eprintln!("Failed to drop from root to {}", actual);
                Err(())
            },
        }
    }

    /// Run a block as root in a forked child
    pub(crate) fn _as_root_with_chroot<F, P>(f: F, root: P) 
        -> Result<(), ()>
    where 
        F: FnOnce() -> Result<(), ()>,
        P: AsRef<Path>
    {
        Self::fork_and_run(||{
            if let Err(e) = chroot(root.as_ref()) {
                eprintln!("Child: Failed to chroot to '{}': {}", 
                    root.as_ref().display(), e);
                return Err(())
            }
            if Self::sete_root().is_err() {
                eprintln!("Child: Failed to seteuid back to root");
                return Err(())
            }
            f()
        })
    }

    pub(crate) fn as_root<F: FnOnce() -> Result<(), ()>>(f: F) -> Result<(), ()>
    {
        Self::fork_and_run(||{
            if Self::sete_root().is_err() {
                eprintln!("Child: Failed to seteuid back to root");
                return Err(())
            }
            f()
        })
    }

    pub(crate) fn _with_chroot<F, P>(f: F, root: P) 
        -> Result<(), ()>
    where 
        F: FnOnce() -> Result<(), ()>,
        P: AsRef<Path>
    {
        Self::fork_and_run(||{
            if let Err(e) = chroot(root.as_ref()) {
                eprintln!("Child: Failed to chroot to '{}': {}", 
                    root.as_ref().display(), e);
                return Err(())
            }
            f()
        })
    }

    pub(crate) fn home(&self) -> Result<PathBuf, ()> {
        if let Some(env) = &self.env {
            Ok(PathBuf::from(&env.home))
        } else {
            eprint!("Failed to get home dir, this should not happen");
            Err(())
        }
    }

    pub(crate) fn user(&self) -> Result<String, ()> {
        if let Some(env) = &self.env {
            if let Some(str) = env.user.to_str() {
                return Ok(str.to_string())
            }
        }
        eprint!("Failed to get user name, this should not happen");
        Err(())
    }

    pub(crate) fn cwd(&self) -> Result<PathBuf, ()> {
        if let Some(env) = &self.env {
            Ok(PathBuf::from(&env.cwd))
        } else {
            eprint!("Failed to get home dir, this should not happen");
            Err(())
        }
    }
}