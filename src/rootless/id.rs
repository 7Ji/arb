use std::fmt::Display;

use nix::{libc::{gid_t, uid_t}, unistd::{getresgid, getresuid, setgid, setuid, Gid, ResGid, ResUid, Uid}};

use crate::{Error, Result};

pub(crate) struct ResUidGid {
    uid: ResUid,
    gid: ResGid,
}

impl ResUidGid {
    pub(crate) fn new() -> Result<Self> {
        let uid = match getresuid() {
            Ok(uid) => uid,
            Err(e) => {
                log::error!("Failed to get current real, effective, saved uid");
                return Err(e.into())
            },
        };
        let gid = match getresgid() {
            Ok(gid) => gid,
            Err(e) => {
                log::error!("Failed to get current real, effective, saved gid");
                return Err(e.into())
            },
        };
        Ok(Self { uid, gid })
    }

    pub(crate) fn is_root(&self) -> bool {
        self.uid.real.is_root() && self.uid.effective.is_root() && 
        self.uid.saved.is_root() && self.gid.real.as_raw() == 0 && 
        self.gid.effective.as_raw() == 0 && self.gid.saved.as_raw() == 0
    }

    /// Is definitely not root, `is_not_root() == true` is more strict than 
    /// `is_root() == false`
    pub(crate) fn is_not_root(&self) -> bool {
        !(self.uid.real.is_root() || self.uid.effective.is_root() || 
        self.uid.saved.is_root() || self.gid.real.as_raw() == 0 || 
        self.gid.effective.as_raw() == 0 || self.gid.saved.as_raw() == 0)
    }
}

impl Display for ResUidGid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "uid: real {}, effective {}, saved {}; \
                   gid: real {}, effective {}, saved {}", 
                self.uid.real, self.uid.effective, self.uid.saved, 
                self.gid.real, self.gid.effective, self.gid.saved)
    }
}

pub(crate) fn ensure_no_root() -> Result<()> {
    if ResUidGid::new()?.is_not_root() {
        Ok(())
    } else {
        log::error!("We're runnning as root!");
        Err(Error::BrokenEnvironment)
    }
}

fn setgid_checked(gid: gid_t) -> Result<()> {
    if let Err(e) = setgid(Gid::from_raw(gid)) {
        log::error!("Failed to set gid to {}: {}", gid, e);
        Err(e.into())
    } else {
        Ok(())
    }
}

fn setuid_checked(uid: uid_t) -> Result<()> {
    if let Err(e) = setuid(Uid::from_raw(uid)) {
        log::error!("Failed to set uid to {}: {}", uid, e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn set_uid_gid(uid: uid_t, gid: gid_t) -> Result<()> {
    setgid_checked(gid)?;
    setuid_checked(uid)
}