use std::{hash::Hasher, process::{Command, Stdio}};

use alpm::{self, Package};
use xxhash_rust::xxh3;

use crate::identity::Identity;

#[derive(Clone)]
pub(super) struct Depends {
    pub(super) deps: Vec<String>,
    pub(super) makedeps: Vec<String>,
    pub(super) needs: Vec<String>,
    pub(super) hash: u64,
}

pub(super) struct DbHandle {
    alpm_handle: alpm::Alpm,
}

impl DbHandle {
    pub(super) fn new<S: AsRef<str>>(root: S) -> Result<Self, ()> {
        let handle = match alpm::Alpm::new(
            root.as_ref(), "/var/lib/pacman") 
        {
            Ok(handle) => handle,
            Err(e) => {
                eprintln!("Failed to open pacman DB at root '{}': {}",
                root.as_ref(), e);
                return Err(())
            },
        };
        let content = match std::fs::read_to_string(
            "/etc/pacman.conf") 
        {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to open pacman config: {}", e);
                return Err(())
            },
        };
        let sig_level = handle.default_siglevel();
        for line in content.lines() {
            let line = line.trim();
            if ! line.starts_with('[') || ! line.ends_with(']') {
                continue   
            }
            let section = line.trim_start_matches('[')
                .trim_end_matches(']');
            if section == "options" {
                continue
            }
            match handle.register_syncdb(section, sig_level) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Failed to register repo '{}': {}", section, e);
                    return Err(())
                },
            }
        }
        if handle.syncdbs().len() == 0 {
            eprintln!("No DBs defined");
            return Err(())
        }
        Ok(DbHandle { alpm_handle: handle })
    }

    fn find_satisfier<S: AsRef<str>>(&self, dep: S) -> Option<Package> {
        let mut pkg_satisfier = None;
        for db in self.alpm_handle.syncdbs() {
            if let Ok(pkg) = db.pkg(dep.as_ref()) {
                return Some(pkg)
            }
            if let Some(pkg) = 
                db.pkgs().find_satisfier(dep.as_ref()) 
            {
                pkg_satisfier = Some(pkg)
            }
        }
        pkg_satisfier
    }

    fn _is_installed<S: AsRef<str>>(&self, pkg: S) -> bool {
        match self.alpm_handle.localdb().pkg(pkg.as_ref()) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

impl Depends {
    pub(super) fn needed_and_hash(&mut self, db_handle: &DbHandle) 
        -> Result<(), ()> 
    {
        let mut hash_box = Box::new(xxh3::Xxh3::new());
        let hash = hash_box.as_mut();
        self.needs.clear();
        for dep in self.deps.iter() {
            let dep = match db_handle.find_satisfier(dep) {
                Some(dep) => dep,
                None => {
                    eprintln!("Warning: dep {} not found", dep);
                    return Err(())
                },
            };
            self.needs.push(dep.name().to_string());
            if let Some(sig) = dep.base64_sig() {
                hash.update(sig.as_bytes());
                continue
            }
            if let Some(sha256) = dep.sha256sum() {
                hash.update(sha256.as_bytes());
                continue
            }
            if let Some(md5) = dep.md5sum() {
                hash.update(md5.as_bytes());
                continue
            }
            // The last resort
            hash.update(dep.name().as_bytes());
            hash.update(dep.version().as_bytes());
            hash.write_i64(dep.build_date());
            // There're of couse other vars, but as we add more of them
            // we will add the possibility of fake-positive
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
        self.needs.sort_unstable();
        self.needs.dedup();
        self.hash = hash.finish();
        // needs.retain(|pkg|!db_handle.is_installed(pkg));
        Ok(())
    }

    pub(super) fn cache_raw<S: AsRef<str>>(deps: &Vec<String>, root: S) 
        -> Result<(), ()> 
    {
        if deps.len() == 0 {
            return Ok(())
        }
        println!("Caching the following dependencies on host: {:?}", deps);
        let output = match Identity::set_root_command(
            Command::new("/usr/bin/pacman")
                .env("LANG", "C")
                .arg("-S")
                .arg("--root")
                .arg(root.as_ref())
                .arg("--noconfirm")
                .arg("--downloadonly")
                .args(deps)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
            ).output()
        {
            Ok(output) => output,
            Err(e) => {
                eprintln!("Failed to spawn child: {}", e);
                return Err(());
            },
        };
        if Some(0) != output.status.code() {
            eprintln!("Download-only command failed to execute correctly");
            return Err(())
        }
        Ok(())
    }

    pub(super) fn _cache<S: AsRef<str>>(&self, root: S) -> Result<(), ()> {
        Self::cache_raw(&self.needs, root)
    }
}