use std::{fmt::Display, path::Path};

use nix::{mount::{mount, MsFlags}, NixPath};

use crate::{Error, Result};

pub(crate) fn mount_checked<
    P1: ? Sized + NixPath,
    P2: ? Sized + NixPath,
    P3: ? Sized + NixPath,
    P4: ? Sized + NixPath,
    S1: Display,
    S2: Display
>(
    source: Option<&P1>,
    target: &P2,
    fstype: Option<&P3>,
    flags: MsFlags,
    data: Option<&P4>,
    source_human_readable: S1,
    target_human_readable: S2
) -> Result<()> 
{
    if let Err(e) =  mount(source, target, fstype, flags, data) {
        log::error!("Failed to mount '{}' to '{}': {}", 
            source_human_readable, target_human_readable, e);
        Err(e.into())
    } else {
        Ok(())
    }
}

pub(crate) fn mount_proc<P: AsRef<Path>>(path_proc: P) -> Result<()> {
    mount_checked(
        Some("proc"),
        path_proc.as_ref(),
        Some("proc"),
        MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
        None::<&str>,
        "proc",
        path_proc.as_ref().display()
    )
}