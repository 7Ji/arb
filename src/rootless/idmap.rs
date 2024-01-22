use std::{fs::File, path::Path, io::{BufRead, BufReader, Read}, ffi::{OsStr, OsString}, os::unix::ffi::OsStrExt, fmt::format, process::Command};

use nix::{unistd::{Uid, getuid, getgid}, libc::{uid_t, gid_t, pid_t}};

use crate::{Error, Result};

#[derive(Debug)]
struct IdMap {
    out_self: uid_t,
    out_sub: uid_t,
}

#[derive(Debug)]
pub(crate) struct IdMaps {
    uid_map: IdMap,
    gid_map: IdMap
}

#[derive(Debug)]
struct SubId {
    start: uid_t,
    range: uid_t
}

impl SubId {
    fn from_file<P: AsRef<Path>, S: AsRef<str>>(path: P, id: uid_t, name: S) 
        -> Result<Self>
    {
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to open subid file '{}': {}", 
                    path.as_ref().display(), e);
                return Err(e.into())
            },
        };
        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            log::error!("Failed to read content of subid file '{}': {}",
                path.as_ref().display(), e);
            return Err(e.into())
        }
        let id = format!("{}", id);
        for line in buffer.split(|c|*c == b'\n')  {
            if line.is_empty() { continue }
            let mut segments = line.split(|c|*c == b':');
            let identifier = segments.next().unwrap();
            if identifier != id.as_bytes() && identifier != name.as_ref().as_bytes() { continue }
            let start = segments.next().unwrap();
            let range = segments.next().unwrap();
            if segments.next().is_some() { panic!("subid file contains illegal line") }
            let range: uid_t = String::from_utf8_lossy(range).parse().unwrap();
            if range >= 65535 {
                return Ok(Self { start: String::from_utf8_lossy(start).parse().unwrap() , range })
            }
        }
        log::error!("Cannot find subid config");
        return Err(Error::InvalidConfig)
    }
}

impl IdMap {
    fn out_self_str(&self) -> String {
        format!("{}", self.out_self)
    }

    fn out_sub_str(&self) -> String {
        format!("{}", self.out_sub)
    }

    fn new(out_self: uid_t, out_sub: uid_t) -> Self {
        Self {
            out_self,
            out_sub,
        }
    }

    fn set_pid(&self, pid: pid_t, prog: &str) {
        if ! Command::new(prog)
            .arg(format!("{}", pid))
            .arg("0")
            .arg(self.out_self_str())
            .arg("1")
            .arg("1")
            .arg(self.out_sub_str())
            .arg("65535")
            .spawn()
            .unwrap()
            .wait_with_output()
            .unwrap()
            .status
            .success() 
        {
            panic!("Failed to set new idmap")
        }
    }
}


impl IdMaps {
    pub(crate) fn new() -> Result<Self> {
        let uid = getuid();
        let passwd = passwd::Passwd::from_uid(uid.as_raw()).unwrap();
        let name = passwd.name;
        let subuid = SubId::from_file("/etc/subuid", uid.as_raw(), &name)?;
        let gid = getgid();
        let subgid = SubId::from_file("/etc/subgid", gid.as_raw(), &name)?;
        return Ok(Self {
            uid_map: IdMap::new(uid.as_raw(), subuid.start),
            gid_map: IdMap::new(gid.as_raw(), subgid.start),
        })
    }

    pub(crate) fn set_pid(&self, pid: pid_t) {
        self.gid_map.set_pid(pid, "/usr/bin/newgidmap");
        self.uid_map.set_pid(pid, "/usr/bin/newuidmap");
    }
}