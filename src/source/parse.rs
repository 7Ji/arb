use std::{
    path::Path,
    process::Command,
};
use xxhash_rust::xxh3::xxh3_64;

use super::{
    cksums::Sum,
    netfile::push_source as push_netfile_source,
    git::push_source as push_git_source,
    Source,
    VcsProtocol,
    Protocol,
    Cksum,
    Md5sum,
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
    B2sum
};

fn push_source(
    sources: &mut Vec<Source>,
    name: Option<String>,
    protocol: Option<Protocol>,
    url: Option<String>,
    hash_url: u64,
    ck: Option<Cksum>,     // 32-bit CRC
    md5: Option<Md5sum>,   // 128-bit MD5
    sha1: Option<Sha1sum>,  // 160-bit SHA-1
    sha224: Option<Sha224sum>,// 224-bit SHA-2
    sha256: Option<Sha256sum>,// 256-bit SHA-2
    sha384: Option<Sha384sum>,// 384-bit SHA-2
    sha512: Option<Sha512sum>,// 512-bit SHA-2
    b2: Option<B2sum>,    // 512-bit Blake-2B
) -> Result<(),()>
{
    if let None = ck {
    if let None = md5 {
    if let None = sha1 {
    if let None = sha224 {
    if let None = sha256 {
    if let None = sha384 {
    if let None = sha512 {
    if let None = b2 {
    if let Some(protocol) = &protocol {
    if let Protocol::Netfile { protocol: _ } = protocol {
        return Ok(()) // Skip netfiles that do not have integ
    }}}}}}}}}}
    if let Some(name) = name {
        if let Some(protocol) = protocol {
            if let Some(url) = url {
                sources.push(Source{
                    name,
                    protocol,
                    url,
                    hash_url,
                    ck,
                    md5,
                    sha1,
                    sha224,
                    sha256,
                    sha384,
                    sha512,
                    b2,
                });
                return Ok(())
            }
        }
    };
    eprintln!("Unfinished source definition");
    Err(())
}

pub(crate) fn get_sources<P> (pkgbuild: P) -> Option<Vec<Source>>
where
    P: AsRef<Path>
{
    const SCRIPT: &str = include_str!("../../scripts/get_sources.bash");
    let output = Command::new("/bin/bash")
        .arg("-ec")
        .arg(SCRIPT)
        .arg("Source reader")
        .arg(pkgbuild.as_ref())
        .output()
        .expect("Failed to run script");
    let mut name = None;
    let mut protocol = None;
    let mut url = None;
    let mut hash_url = 0;
    let mut ck = None;
    let mut md5 = None;
    let mut sha1 = None;
    let mut sha224 = None;
    let mut sha256 = None;
    let mut sha384 = None;
    let mut sha512 = None;
    let mut b2 = None;
    let mut sources = vec![];
    let mut started = false;
    for line in  output.stdout.split(|byte| byte == &b'\n') {
        if line.len() == 0 {
            continue;
        }
        if line == b"[source]" {
            if started {
                push_source(&mut sources,
                    name, protocol, url, hash_url,
                    ck, md5, sha1,
                    sha224, sha256, sha384, sha512,
                    b2).ok()?;
                name = None;
                protocol = None;
                url = None;
                hash_url = 0;
                ck = None;
                md5 = None;
                sha1 = None;
                sha224 = None;
                sha256 = None;
                sha384 = None;
                sha512 = None;
                b2 = None;
            } else {
                started = true;
            }
            continue;
        }
        let mut it =
            line.splitn(2, |byte| byte == &b':');
        let key = it.next().expect("Failed to get key");
        let value = it.next().expect("Failed to get value");
        match key {
            b"name" => {
                name = Some(String::from_utf8_lossy(value).into_owned());
            }
            b"protocol" => {
                if let Some(protocol_parse) = 
                    Protocol::from_raw_string(value) 
                {
                    protocol = Some(protocol_parse);
                }
            }
            b"url" => {
                url = Some(String::from_utf8_lossy(value).into_owned());
                hash_url = xxh3_64(value);
            }
            b"cksum" => ck = Cksum::from_hex(value),
            b"md5sum" => md5 = Md5sum::from_hex(value),
            b"sha1sum" => sha1 = Sha1sum::from_hex(value),
            b"sha224sum" => sha224 = Sha224sum::from_hex(value),
            b"sha256sum" => sha256 = Sha256sum::from_hex(value),
            b"sha384sum" => sha384 = Sha384sum::from_hex(value),
            b"sha512sum" => sha512 = Sha512sum::from_hex(value),
            b"b2sum" => b2 = B2sum::from_hex(value),
            &_ => {
                eprintln!("Unexpected line: {}", String::from_utf8_lossy(line));
                return None
            }
        }
    }
    push_source(&mut sources,
        name, protocol, url, hash_url,
        ck, md5, sha1,
        sha224, sha256, sha384, sha512,
        b2).ok()?;
    Some(sources)
}


pub(crate) fn unique_sources(sources: &Vec<&Source>)
    -> Option<(Vec<Source>, Vec<Source>, Vec<Source>)>
{
    let mut local_sources: Vec<Source> = vec![];
    let mut git_sources: Vec<Source> = vec![];
    let mut netfile_sources: Vec<Source> = vec![];
    for source in sources.iter() {
        match &source.protocol {
            Protocol::Netfile { protocol: _ } => 
                push_netfile_source(&mut netfile_sources, source).ok()?,
            Protocol::Vcs { protocol } => {
                match protocol {  // Ignore VCS sources we do not support
                    VcsProtocol::Bzr => (),
                    VcsProtocol::Fossil => (),
                    VcsProtocol::Git =>
                        push_git_source(&mut git_sources, source),
                    VcsProtocol::Hg => (),
                    VcsProtocol::Svn => (),
                }
            },
            Protocol::Local => local_sources.push(source.to_owned().to_owned())
        }
    }
    Some((netfile_sources, git_sources, local_sources))
}
