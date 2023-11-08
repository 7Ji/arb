// Parse an on-disk 

use std::{path::Path, process::{Command, Stdio}, io::{Write, Read}};

use crate::identity::IdentityActual;

struct PackageBorrowed<'a> {
    name: &'a [u8],
    deps: Vec<&'a [u8]>,
    provides: Vec<&'a [u8]>,
}

impl<'a> Default for PackageBorrowed<'a> {
    fn default() -> Self {
        Self {
            name: b"",
            deps: vec![],
            provides: vec![] 
        }
    }
}

struct PkgbuildBorrowed<'a> {
    base: &'a [u8],
    pkgs: Vec<PackageBorrowed<'a>>,
    deps: Vec<&'a [u8]>,
    makedeps: Vec<&'a [u8]>,
    provides: Vec<&'a [u8]>,
    sources: Vec<&'a [u8]>,
    cksums: Vec<&'a [u8]>,
    md5sums: Vec<&'a [u8]>,
    sha1sums: Vec<&'a [u8]>,
    sha224sums: Vec<&'a [u8]>,
    sha256sums: Vec<&'a [u8]>,
    sha384sums: Vec<&'a [u8]>,
    sha512sums: Vec<&'a [u8]>,
    b2sums: Vec<&'a [u8]>,
    pkgver_func: bool,
}

impl<'a> PkgbuildBorrowed<'a> {
    fn find_pkg_mut(&'a mut self, name: &[u8]) 
        -> Result<&mut PackageBorrowed, ()> 
    {
        let mut pkg = None;
        for pkg_cmp in self.pkgs.iter_mut() {
            if pkg_cmp.name == name {
                pkg = Some(pkg_cmp);
                break
            }
        }
        pkg.ok_or_else(||log::error!("Failed to find pkg {}",
            String::from_utf8_lossy(name)))
    }
    fn push_pkg_dep(&'a mut self, pkg_name: &[u8], dep: &'a [u8])
        -> Result<(), ()> 
    {
        let pkg = self.find_pkg_mut(pkg_name)?;
        pkg.deps.push(dep);
        Ok(())
    }
    fn push_pkg_provide(&'a mut self, pkg_name: &[u8], provide: &'a [u8]) 
        -> Result<(), ()> 
    {
        let pkg = self.find_pkg_mut(pkg_name)?;
        pkg.provides.push(provide);
        Ok(())
    }
}

impl<'a> Default for PkgbuildBorrowed<'a> {
    fn default() -> Self {
        Self {
            base: b"",
            pkgs: vec![],
            deps: vec![],
            makedeps: vec![],
            provides: vec![],
            sources: vec![],
            cksums: vec![],
            md5sums: vec![],
            sha1sums: vec![],
            sha224sums: vec![],
            sha256sums: vec![],
            sha384sums: vec![],
            sha512sums: vec![],
            b2sums: vec![],
            pkgver_func: false,
        }
    }
}

struct PkgbuildsBorrowed<'a> {
    entries: Vec<PkgbuildBorrowed<'a>>
}


impl<'a> PkgbuildsBorrowed<'a> {
    fn from_parser_output(output: &'a Vec<u8>) -> Result<Self, ()> {
        let mut pkgbuilds = vec![];
        let mut pkgbuild = PkgbuildBorrowed::default();
        let mut started = false;
        for line in output.split(|byte| *byte == b'\n') {
            if line.is_empty() { continue }
            if line.contains(&b':') {
                let mut it =
                    line.splitn(2, |byte| byte == &b':');
                let key = it.next().ok_or_else(
                    ||log::error!("Failed to get key"))?;
                let value = it.next().ok_or_else(
                    ||log::error!("Failed to get value"))?;
                match key {
                    b"base" => pkgbuild.base = value,
                    b"name" => {
                        let mut pkg = 
                            PackageBorrowed::default();
                        pkg.name = value;
                        pkgbuild.pkgs.push(pkg);
                    },
                    b"dep" => pkgbuild.deps.push(value),
                    b"makedep" => pkgbuild.makedeps.push(value),
                    b"provide" => pkgbuild.provides.push(value),
                    b"source" => pkgbuild.sources.push(value),
                    b"ck" => pkgbuild.cksums.push(value),
                    b"md5" => pkgbuild.md5sums.push(value),
                    b"sha1" => pkgbuild.sha1sums.push(value),
                    b"sha224" => pkgbuild.sha224sums.push(value),
                    b"sha256" => pkgbuild.sha256sums.push(value),
                    b"sha384" => pkgbuild.sha384sums.push(value),
                    b"sha512" => pkgbuild.sha512sums.push(value),
                    b"b2" => pkgbuild.b2sums.push(value),
                    b"pkgver_func" => match value {
                        b"y" => pkgbuild.pkgver_func = true,
                        b"n" => pkgbuild.pkgver_func = false,
                        _ => {
                            log::error!("Unexpected value: {}", 
                                String::from_utf8_lossy(value));
                            return Err(())
                        }
                    }
                    _ => {
                        let (offset, is_dep) = 
                        if key.starts_with(b"dep_") {(4, true)}
                        else if key.starts_with(b"provide_") {(8, false)}
                        else {
                            log::error!("Unexpected line: {}", 
                                String::from_utf8_lossy(line));
                            return Err(())
                        };
                        let name = &key[offset..];
                        let mut pkg = None;
                        for pkg_cmp in 
                            pkgbuild.pkgs.iter_mut() 
                        {
                            if pkg_cmp.name == name {
                                pkg = Some(pkg_cmp);
                                break
                            }
                        }
                        let pkg = pkg.ok_or_else(
                            ||log::error!("Failed to find pkg {}",
                            String::from_utf8_lossy(name)))?;
                        if is_dep {
                            pkg.deps.push(value)
                        } else {
                            pkg.provides.push(value)
                        }
                    }
                }
            } else if line == b"[PKGBUILD]" {
                if started {
                    pkgbuilds.push(pkgbuild);
                    pkgbuild = PkgbuildBorrowed::default();
                } else {
                    started = true
                }
            } else {
                log::error!("Illegal line: {}", String::from_utf8_lossy(line));
                return Err(())
            }
        }
        pkgbuilds.push(pkgbuild);
        Ok(Self {
            entries: pkgbuilds,
        })
    }
}

struct PackageOwned {
    name: String,
    deps: Vec<String>,
    provides: Vec<String>,
}

struct PkgbuildOwned {
    base: String,
    pkgs: Vec<PackageOwned>,
    deps: Vec<String>,
    makedeps: Vec<String>,
    provides: Vec<String>,
    sources: Vec<String>,
    cksums: Vec<String>,
    md5sums: Vec<String>,
    sha1sums: Vec<String>,
    sha224sums: Vec<String>,
    sha256sums: Vec<String>,
    sha384sums: Vec<String>,
    sha512sums: Vec<String>,
    b2sums: Vec<String>,
    pkgver_func: bool,
}
struct PkgbuildsOwned {
    entries: Vec<PkgbuildOwned>
}

fn vec_string_from_vec_u8(original: &Vec<&[u8]>) -> Vec<String> {
    original.iter().map(|item|
        String::from_utf8_lossy(item).into_owned()).collect()
}

impl PackageOwned {
    fn from_borrowed(borrowed: &PackageBorrowed) -> Self {
        Self {
            name: String::from_utf8_lossy(borrowed.name).into_owned(),
            deps: vec_string_from_vec_u8(&borrowed.deps),
            provides: vec_string_from_vec_u8(&borrowed.provides),
        }
    }
}

impl PkgbuildOwned {
    fn from_borrowed(borrowed: &PkgbuildBorrowed) -> Self {
        Self {
            base: String::from_utf8_lossy(borrowed.base).into_owned(),
            pkgs: borrowed.pkgs.iter().map(|pkg|
                PackageOwned::from_borrowed(pkg)).collect(),
            deps: vec_string_from_vec_u8(&borrowed.deps),
            makedeps: vec_string_from_vec_u8(&borrowed.makedeps),
            provides: vec_string_from_vec_u8(&borrowed.provides),
            sources: vec_string_from_vec_u8(&borrowed.sources),
            cksums: vec_string_from_vec_u8(&borrowed.cksums),
            md5sums: vec_string_from_vec_u8(&borrowed.md5sums),
            sha1sums: vec_string_from_vec_u8(&borrowed.sha1sums),
            sha224sums: vec_string_from_vec_u8(&borrowed.sha224sums),
            sha256sums: vec_string_from_vec_u8(&borrowed.sha256sums),
            sha384sums: vec_string_from_vec_u8(&borrowed.sha384sums),
            sha512sums: vec_string_from_vec_u8(&borrowed.sha512sums),
            b2sums: vec_string_from_vec_u8(&borrowed.b2sums),
            pkgver_func: borrowed.pkgver_func,
        }
    }
}

impl PkgbuildsOwned {
    fn from_borrowed(borrowed: PkgbuildsBorrowed) -> Self {
        Self {
            entries: borrowed.entries.iter().map(
                |entry|
                    PkgbuildOwned::from_borrowed(entry)).collect(),
        }
    }
}

impl PkgbuildsOwned {
    fn from_dumped_pkgbuilds<P, I, S> (
        dir: P, list: I, actual_identity: IdentityActual
    ) -> Result<Self, ()>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut write_buffer = vec![];
        for pkgbuild_name in list.into_iter() {
            for byte in pkgbuild_name.as_ref().as_bytes() {
                write_buffer.push(*byte)
            }
            write_buffer.push(b'\n');
        }
        let mut child = match actual_identity.set_root_drop_command(
            Command::new("/bin/bash")
                .arg("-c")
                .arg(include_str!("../../scripts/parse_pkgbuilds.bash"))
                .arg("PKGBUILD Parser")
                .current_dir(dir.as_ref())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped()))
                .stderr(Stdio::null())
            .spawn() 
        {
            Ok(child) => child,
            Err(e) => {
                log::error!("Failed to spawn child to parse pkgbuilds: {}", e);
                return Err(())
            },
        };
        let mut child_in = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                log::error!("Failed to open stdin");
                child.kill().expect("Failed to kill child");
                return Err(())
            },
        };
        let mut child_out = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                log::error!("Failed to open stdin");
                child.kill().expect("Failed to kill child");
                return Err(())
            },
        };
        let mut output = vec![];
        let mut output_buffer = vec![0; libc::PIPE_BUF];
        let mut written = 0;
        let total = write_buffer.len();
        while written < total {
            let mut end = written + libc::PIPE_BUF;
            if end > total {
                end = total;
            }
            match child_in.write(&write_buffer[written..end]) {
                Ok(written_this) => written += written_this,
                Err(e) => {
                    log::error!("Failed to write buffer to child: {}", e);
                    child.kill().expect("Failed to kill child");
                    return Err(())
                },
            }
            match child_out.read(&mut output_buffer[..]) {
                Ok(read_this) => 
                    output.extend_from_slice(&output_buffer[0..read_this]),
                Err(e) => {
                    log::error!("Failed to read stdout child: {}", e);
                    child.kill().expect("Failed to kill child");
                    return Err(())
                },
            }
        }
        drop(child_in);
        match child_out.read_to_end(&mut output) {
            Ok(_) => (),
            Err(e) => {
                log::error!("Failed to read stdout child: {}", e);
                child.kill().expect("Failed to kill child");
                return Err(())
            },
        }
        if child
            .wait()
            .or_else(|e|{
                log::error!(
                    "Failed to wait for child parsing PKGBUILDs: {}", e);
                Err(())
            })?
            .code()
            .ok_or_else(||{
                log::error!("Failed to get return code from child parsing \
                        PKGBUILD type")
            })? != 0 {
                log::error!("Reader bad return");
                return Err(())
            }
        Ok(Self::from_borrowed(PkgbuildsBorrowed::from_parser_output(&output)?))
    }
}