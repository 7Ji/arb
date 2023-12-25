// TODO: Use native openpgp implementation
use std::{
        fs::read_dir,
        path::Path,
        process::{
            Command,
            Stdio,
        },
    };

use crate::error::{
        Error,
        Result
    };

fn sign_pkg(file: &Path, key: &str)
    -> Result<()>
{
    crate::child::output_and_check(
        actual_identity.set_root_drop_command(
            Command::new("/usr/bin/gpg")
                .arg("--detach-sign")
                .arg("--local-user")
                .arg(key)
                .arg(file))
                .stdin(Stdio::null()),
        "to sign pkg"
    )
}

pub(crate) fn sign_pkgs(actual_identity: &IdentityActual, dir: &Path, key: &str)
    -> Result<()>
{
    let reader = match read_dir(dir) {
        Ok(reader) => reader,
        Err(e) => {
            log::error!("Failed to read temp pkgdir: {}", e);
            return Err(Error::IoError(e))
        },
    };
    let mut bad = false;
    for entry in reader {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("Failed to read entry from temp pkgdir: {}", e);
                bad = true;
                continue
            },
        }.path();
        if entry.ends_with(".sig") {
            continue
        }
        if sign_pkg(actual_identity, &entry, key).is_err() { bad = true }
    }
    if bad { Err(Error::BadChild { pid: None, code: None }) } else { Ok(()) }
}