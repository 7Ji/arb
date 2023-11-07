use std::{hash::Hasher, ffi::OsStr, process::{Command, Stdio}};

use alpm::Package;
use xxhash_rust::xxh3;

use crate::{config::DepHashStrategy, identity::{IdentityActual, Identity}};

use super::DbHandle;


#[derive(Clone)]
pub(crate) struct Depends {
    pub(crate) deps: Vec<String>,
    pub(crate) makedeps: Vec<String>,
    pub(crate) needs: Vec<String>,
    pub(crate) hash: u64,
}


fn update_hash_from_pkg(hash: &mut xxh3::Xxh3, pkg: Package<'_>) {
    if let Some(sig) = pkg.base64_sig() {
        hash.update(sig.as_bytes());
        return
    }
    if let Some(sha256) = pkg.sha256sum() {
        hash.update(sha256.as_bytes());
        return
    }
    if let Some(md5) = pkg.md5sum() {
        hash.update(md5.as_bytes());
        return
    }
    // The last resort
    hash.update(pkg.name().as_bytes());
    hash.update(pkg.version().as_bytes());
    hash.write_i64(pkg.build_date());
    // There're of couse other vars, but as we add more of them
    // we will add the possibility of fake-positive
}

impl Depends {
    fn needed_and_strict_hash(&mut self, db_handle: &DbHandle) -> Result<(), ()>
    {
        let mut hash_box = Box::new(xxh3::Xxh3::new());
        let hash = hash_box.as_mut();
        for dep in self.deps.iter().chain(self.makedeps.iter()) {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    eprintln!("Warning: dep {} not found", dep);
                    return Err(())
                },
            };
            self.needs.push(dep.name().to_string());
            update_hash_from_pkg(hash, dep);
        }
        self.hash = hash.finish();
        Ok(())
    }

    fn needed_and_loose_hash(&mut self, db_handle: &DbHandle) -> Result<(), ()>
    {
        let mut hash_box = Box::new(xxh3::Xxh3::new());
        let hash = hash_box.as_mut();
        for dep in self.deps.iter() {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    eprintln!("Warning: dep {} not found", dep);
                    return Err(())
                },
            };
            self.needs.push(dep.name().to_string());
            update_hash_from_pkg(hash, dep);
        }
        for dep in self.makedeps.iter() {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    eprintln!("Warning: dep {} not found", dep);
                    return Err(())
                },
            };
            self.needs.push(dep.name().to_string());
        }
        self.hash = hash.finish();
        Ok(())
    }

    fn needed_and_no_hash(&mut self, db_handle: &DbHandle) -> Result<(), ()> {
        for dep in self.deps.iter().chain(self.makedeps.iter()) {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    eprintln!("Warning: dep {} not found", dep);
                    return Err(())
                },
            };
            self.needs.push(dep.name().to_string());
        }
        self.hash = 0;
        Ok(())
    }

    pub(crate) fn needed_and_hash(
        &mut self, db_handle: &DbHandle, hash_strategy: &DepHashStrategy
    ) 
        -> Result<(), ()> 
    {
        self.needs.clear();
        let r = match hash_strategy {
            DepHashStrategy::Strict => self.needed_and_strict_hash(db_handle),
            DepHashStrategy::Loose => self.needed_and_loose_hash(db_handle),
            DepHashStrategy::None => self.needed_and_no_hash(db_handle),
        };
        self.needs.sort_unstable();
        self.needs.dedup();
        r
    }

    pub(crate) fn update_needed(&mut self, db_handle: &DbHandle) 
    {
        self.needs.retain(|pkg|!db_handle.is_installed(pkg));
    }

    /// Todo: cache package in our own storage, not tainting host, also without
    /// root permission.
    pub(crate) fn cache_raw<S: AsRef<OsStr>>(deps: &Vec<String>, dbpath: S) 
        -> Result<(), ()> 
    {
        if deps.len() == 0 {
            return Ok(())
        }
        println!("Caching the following dependencies on host: {:?}", deps);
        let mut command = Command::new("/usr/bin/pacman");
        IdentityActual::set_root_command(
            &mut command
                .env("LANG", "C")
                .arg("-S")
                .arg("--dbpath")
                .arg(dbpath.as_ref())
                .arg("--noconfirm")
                .arg("--downloadonly")
                .args(deps)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
            );
        let output = match command.output()
        {
            Ok(output) => output,
            Err(e) => {
                eprintln!("Failed to spawn child: {}", e);
                return Err(());
            },
        };
        match output.status.code() {
            Some(code) => match code {
                0 => Ok(()),
                1 => {
                    eprintln!(
                        "Download-only command failed to execute correctly, \
                        bad return 1, maybe due to broken packages? Retrying");
                    let output = match command.output()
                    {
                        Ok(output) => output,
                        Err(e) => {
                            eprintln!("Failed to spawn child: {}", e);
                            return Err(());
                        },
                    };
                    if let Some(0) = output.status.code() {
                        Ok(())
                    } else {
                        eprintln!("Download-only command failed to execute \
                                    correctly");
                        Err(())
                    }
                },
                _ => {
                    eprintln!(
                        "Download-only command failed to execute correctly, \
                        bad return {}", code);
                    Err(())
                }
            },
            None => {
                eprintln!("Download-only command failed to execute correctly");
                Err(())
            },
        }
    }

    pub(crate) fn wants(&self, pkg: &str) -> bool {
        for dep in self.deps.iter().chain(self.makedeps.iter()) {
            if dep == pkg {
                return true
            }
        }
        false
    }
}