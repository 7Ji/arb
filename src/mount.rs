use std::{fmt::Display, fs::{create_dir, read_link}, os::unix::fs::symlink, path::{Path, PathBuf}};

use nix::{libc::MS_NOSUID, mount::{mount, MsFlags}, NixPath};

use crate::{filesystem::touch, Result};

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
    let path_proc = path_proc.as_ref();
    mount_checked(
        Some("proc"),
        path_proc,
        Some("proc"),
        MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_NODEV,
        None::<&str>,
        "proc",
        path_proc.display()
    )
}

fn mount_devpts<P: AsRef<Path>>(path_devpts: P) -> Result<()> {
    let path_devpts = path_devpts.as_ref();
    mount_checked(
        Some("devpts"),
        path_devpts,
        Some("devpts"),
        MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC,
        Some("mode=0620,gid=5"),
        "devpts",
        path_devpts.display()
    )
}

fn mount_tmpfs<P: AsRef<Path>>(
    path_tmpfs: P, name: &str, flags: MsFlags, data: &str
) -> Result<()> 
{
    let path_tmpfs = path_tmpfs.as_ref();
    mount_checked(Some(name),
        path_tmpfs,
        Some("tmpfs"),
        flags,
        Some(data),
        "tmpfs",
        path_tmpfs.display()
    )
}

fn mount_devshm<P: AsRef<Path>>(path_devshm: P) -> Result<()> {
    mount_tmpfs(path_devshm, "shm", 
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV, "mode=1777")
}

fn mount_run<P: AsRef<Path>>(path_run: P) -> Result<()> {
    mount_tmpfs(path_run, "run", 
        MsFlags::MS_NOSUID | MsFlags::MS_NODEV, "mode=0755")
}

fn mount_tmp<P: AsRef<Path>>(path_tmp: P) -> Result<()> {
    mount_tmpfs(path_tmp, "tmp", 
        MsFlags::MS_STRICTATIME | MsFlags::MS_NODEV | MsFlags::MS_NOSUID,
        "mode=1777")
}

pub(crate) fn mount_bind<P1: AsRef<Path>, P2: AsRef<Path>>(source: P1, target: P2) 
    -> Result<()> 
{
    let source = source.as_ref();
    let target = target.as_ref();
    mount_checked(Some(source), target,
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>,
                source.display(),
                target.display())
}

/// This is not actually mounting, but pretending to be a /dev
fn mount_dev<P: AsRef<Path>>(path_dev: P) -> Result<()> {
    let path_dev_target = path_dev.as_ref();
    mount_tmpfs(&path_dev_target, "devtmpfs", MsFlags::MS_NOSUID, "")?;
    let path_dev_source = PathBuf::from("/dev");
    for target in &["full", "null", "random", "tty", "urandom", "zero"] {
        let path_device_target = path_dev_target.join(target);
        let path_device_source = path_dev_source.join(target);
        touch(&path_device_target)?;
        mount_bind(&path_device_source, &path_device_target)?;
    }
    symlink("/proc/self/fd/2", path_dev_target.join("stderr"))?;
    symlink("/proc/self/fd/1", path_dev_target.join("stdout"))?;
    symlink("/proc/self/fd/0", path_dev_target.join("stdin"))?;
    symlink("/proc/kcore", path_dev_target.join("core"))?;
    symlink("/proc/self/fd", path_dev_target.join("fd"))?;
    symlink("pts/ptmx", path_dev_target.join("ptmx"))?;
    symlink(read_link("/dev/stdout")?, path_dev_target.join("console"))?;
    let path_devpts = path_dev_target.join("pts");
    create_dir(&path_devpts)?;
    mount_devpts(&path_devpts)?;
    let path_devshm = path_dev_target.join("shm");
    create_dir(&path_devshm)?;
    mount_devshm(&path_devshm)?;
    Ok(())
}

pub(crate) fn mount_all_except_proc<P: AsRef<Path>>(root: P) -> Result<()> {
    let root = root.as_ref();
    mount_dev(root.join("dev"))?;
    mount_tmp(root.join("tmp"))?;
    mount_run(root.join("run"))?;
    Ok(())
}

pub(crate) fn mount_all<P: AsRef<Path>>(root: P) -> Result<()> {
    let root = root.as_ref();
    mount_proc(root.join("proc"))?;
    mount_devpts(root.join("dev/pts"))?;
    mount_devshm(root.join("dev/shm"))?;
    mount_tmp(root.join("tmp"))?;
    mount_run(root.join("run"))?;
    mount_dev(root.join("dev"))?;
    Ok(())
}