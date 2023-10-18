use std::{path::Path, fs::read_dir, process::{Command, Stdio}};

use crate::identity::IdentityActual;

pub(crate) fn sign_pkgs(actual_identity: &IdentityActual, dir: &Path, key: &str) 
    -> Result<(), ()> 
{
    let reader = match read_dir(dir) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("Failed to read temp pkgdir: {}", e);
            return Err(())
        },
    };
    let mut bad = false;
    for entry in reader {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("Failed to read entry from temp pkgdir: {}", e);
                bad = true;
                continue
            },
        }.path();
        if entry.ends_with(".sig") {
            continue
        }
        let output = match actual_identity.set_root_drop_command(
            Command::new("/usr/bin/gpg")
            .arg("--detach-sign")
            .arg("--local-user")
            .arg(key)
            .arg(&entry))
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output() {
                Ok(output) => output,
                Err(e) => {
                    eprintln!("Failed to spawn child to sign pkg: {}", e);
                    continue
                },
            };
        if Some(0) != output.status.code() {
            eprintln!("Bad return from gpg");
            bad = true
        }
    }
    if bad { Err(()) } else { Ok(()) }
}