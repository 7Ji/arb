// Todo: drop all of this, use user namespaces, so we can operate fully without
// jumping back and forth between root and normal users
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
        let passwd = passwd_from_uid_checked(uid)?;
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