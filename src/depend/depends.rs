use std::{
        ffi::OsStr,
        hash::Hasher,
        process::Command,
    };

use alpm::Package;

use xxhash_rust::xxh3;

use crate::{
        child::output_and_check,
        depend::{
            DbHandle,
            DepHashStrategy,
        },
        error::{
            Error,
            Result,
        },
        identity::{
            Identity,
            IdentityActual,
        },
    };


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
    fn needed_and_strict_hash(&mut self, db_handle: &DbHandle) -> Result<()>
    {
        let mut hash_box = Box::new(xxh3::Xxh3::new());
        let hash = hash_box.as_mut();
        for dep in self.deps.iter().chain(self.makedeps.iter()) {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    log::error!("Warning: dep {} not found", dep);
                    return Err(Error::DependencyMissing(vec![dep.as_str().into()]))
                },
            };
            self.needs.push(dep.name().to_string());
            update_hash_from_pkg(hash, dep);
        }
        self.hash = hash.finish();
        Ok(())
    }

    fn needed_and_loose_hash(&mut self, db_handle: &DbHandle) -> Result<()>
    {
        let mut hash_box = Box::new(xxh3::Xxh3::new());
        let hash = hash_box.as_mut();
        for dep in self.deps.iter() {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    log::error!("Warning: dep {} not found", dep);
                    return Err(Error::DependencyMissing(vec![dep.as_str().into()]))
                },
            };
            self.needs.push(dep.name().to_string());
            update_hash_from_pkg(hash, dep);
        }
        for dep in self.makedeps.iter() {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    log::error!("Warning: dep {} not found", dep);
                    return Err(Error::DependencyMissing(vec![dep.as_str().into()]))
                },
            };
            self.needs.push(dep.name().to_string());
        }
        self.hash = hash.finish();
        Ok(())
    }

    fn needed_and_no_hash(&mut self, db_handle: &DbHandle) -> Result<()> {
        for dep in self.deps.iter().chain(self.makedeps.iter()) {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    log::error!("Warning: dep {} not found", dep);
                    return Err(Error::DependencyMissing(vec![dep.as_str().into()]))
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
        -> Result<()>
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
        -> Result<()>
    {
        if deps.len() == 0 {
            return Ok(())
        }
        log::info!("Caching the following dependencies on host: {:?}", deps);

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
            );
        if let Err(e) = output_and_check(&mut command,
            "to download packages on host") 
        {
            if let Error::BadChild { pid: _, code: Some(1) } = e {
                return output_and_check(&mut command,
                    "to retry to download packages on host")
            }
            Err(e)
        } else {
            Ok(())
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