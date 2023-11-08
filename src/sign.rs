use std::{path::Path, fs::read_dir, process::{Command, Stdio}};

use crate::identity::IdentityActual;

fn sign_pkg(actual_identity: &IdentityActual, file: &Path, key: &str) 
    -> Result<(), ()> 
{
    let output = match actual_identity.set_root_drop_command(
        Command::new("/usr/bin/gpg")
        .arg("--detach-sign")
        .arg("--local-user")
        .arg(key)
        .arg(file))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output() {
            Ok(output) => output,
            Err(e) => {
                log::error!("Failed to spawn child to sign pkg: {}", e);
                return Err(())
            },
        };
    if Some(0) != output.status.code() {
        log::error!("Bad return from gpg");
        Err(())
    } else {
        Ok(())
    }
}

pub(crate) fn sign_pkgs(actual_identity: &IdentityActual, dir: &Path, key: &str) 
    -> Result<(), ()> 
{
    let reader = match read_dir(dir) {
        Ok(reader) => reader,
        Err(e) => {
            log::error!("Failed to read temp pkgdir: {}", e);
            return Err(())
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
    if bad { Err(()) } else { Ok(()) }
}