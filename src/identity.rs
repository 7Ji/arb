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
        }, fmt::Display,
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
pub(crate) struct ForkedChild {
    pid: libc::pid_t
}

impl ForkedChild {
    pub(crate) fn wait(&self) -> Result<(), ()> {
        let mut status: libc::c_int = 0;
        let waited_pid = unsafe {
            libc::waitpid(self.pid, &mut status, 0)
        };
        if waited_pid <= 0 {
            eprintln!("Failed to wait for child: {}", 
                std::io::Error::last_os_error());
            return Err(())
        }
        if waited_pid != self.pid {
            eprintln!("Waited child {} is not the child {} we forked", 
                        waited_pid, self.pid);
            return Err(())
        }
        if status != 0 {
            eprintln!("Child process failed");
            return Err(())
        }
        Ok(())
    }

    pub(crate) fn wait_noop(&self) -> Result<Option<Result<(), ()>>, ()> {
        let mut status: libc::c_int = 0;
        let waited_pid = unsafe {
            libc::waitpid(self.pid, &mut status, libc::WNOHANG)
        };
        if waited_pid < 0 {
            eprintln!("Failed to wait for child: {}", 
                std::io::Error::last_os_error());
            return Err(())
        } else if waited_pid == 0 {
            return Ok(None)
        }
        if waited_pid != self.pid {
            eprintln!("Waited child {} is not the child {} we forked", 
                        waited_pid, self.pid);
            return Err(())
        }
        if status != 0 {
            eprintln!("Child process failed");
            return Ok(Some(Err(())))
        }
        Ok(Some(Ok(())))
    }
}

#[derive(Clone)]
pub(crate) struct IdentityCurrent {
    uid: u32,
    gid: u32,
    name: String,
}

#[derive(Clone)]
pub(crate) struct IdentityActual {
    uid: u32,
    gid: u32,
    name: String,
    env: Environment,
    cwd: PathBuf,
    cwd_no_root: PathBuf,
    user: String,
    home_path: PathBuf,
    home_string: String
}

pub(crate) trait Identity {
    fn uid(&self) -> u32;
    fn gid(&self) -> u32;
    fn name(&self) -> &str;
    fn is_root(&self) -> bool {
        self.uid() == 0 && self.gid() == 0
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
        Self::sete_raw(self.uid(), self.gid())
    }

    fn sete_root() -> Result<(), std::io::Error> {
        Self::sete_raw(0, 0)
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
                eprintln!("Failed to spawn chroot command {:?}: {}", 
                    command, e);
                Err(())
            })?
            .status
            .code()
            .ok_or_else(||{
                eprintln!("Failed to get exit code for chroot command {:?}",
                            command);
                ()
            })?;
        if r == 0 {
            Ok(())
        } else {
            eprintln!("Bad return {} from chroot command {:?}", r, command);
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

    fn fork_and_run_child<F: FnOnce() -> Result<(), ()>,>(f: F)  
        -> Result<ForkedChild, ()>
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
        Ok(ForkedChild{ pid: child })
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

    fn as_root<F: FnOnce() -> Result<(), ()>>(f: F) -> Result<(), ()>
    {
        Self::fork_and_run(||{
            if Self::sete_root().is_err() {
                eprintln!("Child: Failed to seteuid back to root");
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
                eprintln!("Child: Failed to seteuid back to root");
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
                eprintln!("Child: Failed to chroot to '{}': {}", 
                    root.as_ref().display(), e);
                return Err(())
            }
            f()
        })
    }
}

impl Identity for IdentityCurrent {
    fn uid(&self) -> u32 {
        self.uid
    }

    fn gid(&self) -> u32 {
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
    pub(crate) fn new() -> Self {
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
        }
    }
}

impl Identity for IdentityActual {
    fn uid(&self) -> u32 {
        self.uid
    }

    fn gid(&self) -> u32 {
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
    pub(crate) fn user(&self) -> &str {
        &self.user
    }
    pub(crate) fn home_path(&self) -> &Path {
        &self.home_path
    }
    pub(crate) fn home_str(&self) -> &str {
        &self.home_string
    }
    pub(crate) fn new() -> Result<Self, ()> {
        if let Some(sudo_uid) = std::env::var_os("SUDO_UID") {
        if let Some(sudo_gid) = std::env::var_os("SUDO_GID") {
        if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
        if let Ok(uid) = sudo_uid.to_string_lossy().parse() {
        if let Ok(gid) = sudo_gid.to_string_lossy().parse() {
            let name = sudo_user.to_string_lossy().to_string();
            let env = Environment::init(uid, 
                &name).ok_or_else(||{
                    println!("Failed to get env for actual user")
                })?;     
            let cwd = PathBuf::from(&env.cwd);
            let cwd_no_root = cwd.strip_prefix("/").or_else(|e|{
                eprintln!("Failed to strip leading / from cwd: {}", e);
                Err(())
            })?.to_path_buf();
            // let cwd_absolute = cwd.canonicalize().or_else(|e|
            // {
            //     eprintln!("Failed to canonicalize cwd: {}", e);
            //     Err(())
            // })?;
            let user = env.user.to_string_lossy().to_string();
            let home_path = PathBuf::from(&env.home);
            let home_string = env.home.to_string_lossy().to_string();
            return Ok(Self {
                uid,
                gid,
                name: sudo_user.to_string_lossy().to_string(),
                env,
                cwd,
                cwd_no_root,
                // cwd_absolute,
                user,
                home_path,
                home_string
            })
        }}}}}
        Err(())
    }

    pub(crate) fn drop(&self) -> Result<&Self, ()> {
        let current = IdentityCurrent::new();
        if ! current.is_root() {
            eprintln!("Current user is not root, please run builder with sudo");
            return Err(())
        }
        if self.is_root() {
            eprintln!("Actual user is root, please run builder with sudo");
            return Err(())
        }
        match self.sete() {
            Ok(_) => {
                println!("Dropped from root to {}", self);
                Ok(self)
            },
            Err(_) => {
                eprintln!("Failed to drop from root to {}", self);
                Err(())
            },
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
            command.pre_exec(move || Self::set_raw(uid, gid));
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