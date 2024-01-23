use std::{fs::File, path::Path, io::{BufRead, BufReader, Read}, ffi::{OsStr, OsString}, os::unix::ffi::OsStrExt, fmt::{format, Display}, process::Command};

use nix::{unistd::{Uid, getuid, getgid}, libc::{uid_t, gid_t, pid_t}};

use crate::{Error, Result};

// Assumption: uid_t == gid_t, on x86_64 they're both u32, on aarch64 both i32

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
    fn parse_identifier(segment: Option<&[u8]>) -> Result<&[u8]> {
        match segment {
            Some(identifier) => Ok(identifier),
            None => {
                log::error!("No identifier in idmap line");
                Err(Error::InvalidConfig)
            },
        }
    }
    fn parse_segment(segment: Option<&[u8]>) -> Result<uid_t> {
        match segment {
            Some(num) => match String::from_utf8_lossy(num).parse() {
                Ok(num) => Ok(num),
                Err(e) => {
                    log::error!("Subid segment could not be parsed into id");
                    Err(Error::InvalidConfig)
                },
            },
            None => {
                log::error!("Subid missing a segment");
                Err(Error::InvalidConfig)
            },
        }
    }

    fn parse_line(line: &[u8], id: &[u8], name: &[u8])
        -> Result<Option<(uid_t, uid_t)>>
    {
        let mut segments 
            = line.split(|c|*c == b':');
        let identifier = 
            Self::parse_identifier(segments.next())?;
        if identifier != id && identifier != name
        { 
            return Ok(None)
        }
        let start = Self::parse_segment(segments.next())?;
        let range = Self::parse_segment(segments.next())?;
        return Ok(Some((start, range)))
    }

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
        let id_str = format!("{}", id);
        let id_bytes = id_str.as_bytes();
        let name_bytes = name.as_ref().as_bytes();
        for line in buffer.split(|c|*c == b'\n')  {
            if line.is_empty() { continue }
            match Self::parse_line(line, id_bytes, name_bytes) {
                Ok(result) => 
                    if let Some((start, range)) = result {
                        if range >= 65535 {
                            return Ok(Self {start, range})
                        }
                    },
                Err(_) => {
                    log::error!("Idmap file '{}' contains a line that could \
                        not be parsed: {}",  path.as_ref().display(), 
                        String::from_utf8_lossy(line));
                    return Err(Error::InvalidConfig)
                },
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

    fn set_pid(&self, pid: pid_t, prog: &str) -> Result<()> {
        let output = match Command::new(prog)
            .arg(format!("{}", pid))
            .arg("0")
            .arg(self.out_self_str())
            .arg("1")
            .arg("1")
            .arg(self.out_sub_str())
            .arg("65535")
            .output()
        {
            Ok(output) => output,
            Err(e) => {
                log::error!("Failed to spawn child to setid: {}", e);
                return Err(e.into())
            },
        };
        if output.status.success() {
            Ok(())
        } else {
            log::error!("Failed to map ids for pid {}", pid);
            Err(Error::MappingFailure)
        }
    }
}

impl IdMaps {
    /// Get a pair of uidmap and gidmap for the current user.  
    /// 
    /// Namely this picks the first line in `/etc/subuid` that's available to
    /// the current user, and has a range of at least `65535`, and similarly
    /// the first match from `/etc/subgid`.
    /// 
    /// In the worst case scenario this would need to go through all lines.
    /// 
    /// Due to possible performance hit, it's recommended to run this only once
    /// and clone later for re-use, rather than call this every time you need
    /// it.
    pub(crate) fn new() -> Result<Self> {
        let uid = getuid();
        let passwd = match passwd::Passwd::from_uid(uid.as_raw()) {
            Some(passwd) => passwd,
            None => {
                log::error!("Failed to get passwd entry for current user");
                return Err(Error::InvalidConfig)
            },
        };
        let name = passwd.name;
        let subuid = match 
            SubId::from_file("/etc/subuid", uid.as_raw(), &name) 
        {
            Ok(subuid) => subuid,
            Err(e) => {
                log::error!("Failed to get subuid map: {}", e);
                return Err(e)
            },
        };
        let gid = getgid();
        let subgid = match 
            SubId::from_file("/etc/subgid", gid.as_raw(), &name) 
        {
            Ok(subgid) => subgid,
            Err(e) => {
                log::error!("Failed to get subgid map: {}", e);
                return Err(e)
            }
        };
        Ok(Self {
            uid_map: IdMap::new(uid.as_raw(), subuid.start),
            gid_map: IdMap::new(gid.as_raw(), subgid.start),
        })
    }

    /// Map UIDs and GIDs for a certain process with `pid`
    pub(crate) fn set_pid(&self, pid: pid_t) -> Result<()> {
        self.gid_map.set_pid(pid, "/usr/bin/newgidmap")?;
        self.uid_map.set_pid(pid, "/usr/bin/newuidmap")
    }
}

impl Display for IdMaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "uid {} -> 0, {}+65535 -> 1+65535; gid {} -> 0, \
            {}+65535 -> 1+65535", self.uid_map.out_self, self.uid_map.out_sub, 
            self.gid_map.out_self, self.gid_map.out_sub)
    }
}