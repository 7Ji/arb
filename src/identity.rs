use std::{
        os::unix::process::CommandExt,
        process::Command,
    };

#[derive(Clone)]
pub(crate) struct Identity {
    uid: u32,
    gid: u32,
    name: String
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
            }
        }
    }

    pub(crate) fn acutal() -> Self {
        if let Some(sudo_uid) = std::env::var_os("SUDO_UID") {
        if let Some(sudo_gid) = std::env::var_os("SUDO_GID") {
        if let Some(sudo_user) = std::env::var_os("SUDO_USER") {
        if let Ok(uid) = sudo_uid.to_string_lossy().parse() {
        if let Ok(gid) = sudo_gid.to_string_lossy().parse() {
            return Self {
                uid,
                gid,
                name: sudo_user.to_string_lossy().to_string()
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
        Self::current().is_root() && !Self::acutal().is_root()
    }

    fn current_and_actual() -> (Self, Self) {
        (Self::current(), Self::acutal())
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

    pub(crate) fn set_command<'a>(&self, command: &'a mut Command) 
        -> &'a mut Command 
    {
        let pw_dir = unsafe {
            let pw_dir = libc::getpwuid(self.uid).read().pw_dir;
            let len_dir = libc::strlen(pw_dir);
            std::slice::from_raw_parts(pw_dir as *const u8, len_dir)
        };
        command.env_clear()
            .env("SHELL", "/bin/bash")
            .env("PWD", std::env::current_dir()
                .expect("Failed to get current dir"))
            .env("LOGNAME", &self.name)
            .env("HOME", String::from_utf8_lossy(pw_dir).to_string())
            .env("LANG", "en_US.UTF-8")
            .env("USER", &self.name)
            .env("PATH", std::env::var("PATH")
                .expect("Failed to get PATH"));
        Self::set_root_command(command);
        let uid = self.uid;
        let gid = self.gid;
        unsafe {
            command.pre_exec(move || Self::set_raw(uid, gid));
        }
        command
    }

    pub(crate) fn set_root_command(command: &mut Command) -> &mut Command {
        unsafe {
            command.pre_exec(move || Self::sete_root());
        }
        command
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
}