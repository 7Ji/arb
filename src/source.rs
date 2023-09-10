use std::{path::{Path, PathBuf}, process::Command, fmt::Display};
use hex::FromHex;
use xxhash_rust::xxh3::xxh3_64;

use crate::cksums;


#[derive(Debug, Clone)]
enum NetfileProtocol {
    File,
    Ftp,
    Http,
    Https,
    Rsync,
    Scp,
}

#[derive(Debug, Clone)]
enum VcsProtocol {
    Bzr,
    Fossil,
    Git,
    Hg,
    Svn,
}

#[derive(Debug, Clone)]
enum Protocol {
    Netfile {
        protocol: NetfileProtocol
    },
    Vcs {
        protocol: VcsProtocol
    },
    Local
}

impl Protocol {
    fn from_string(value: &str) -> Protocol {
        match value {
            "file" => Protocol::Netfile { protocol: NetfileProtocol::File },
            "ftp" => Protocol::Netfile { protocol: NetfileProtocol::Ftp },
            "http" => Protocol::Netfile { protocol: NetfileProtocol::Http },
            "https" => Protocol::Netfile { protocol: NetfileProtocol::Https },
            "rsync" => Protocol::Netfile { protocol: NetfileProtocol::Rsync },
            "scp" => Protocol::Netfile { protocol: NetfileProtocol::Scp },
            "bzr" => Protocol::Vcs { protocol: VcsProtocol::Bzr },
            "fossil" => Protocol::Vcs { protocol: VcsProtocol::Fossil },
            "git" => Protocol::Vcs { protocol: VcsProtocol::Git },
            "hg" => Protocol::Vcs { protocol: VcsProtocol::Hg },
            "svn" => Protocol::Vcs { protocol: VcsProtocol::Svn },
            "local" => Protocol::Local,
            &_ => {
                eprintln!("Unknown protocol {}", value);
                panic!("Unknown protocol");
            },
        }
    }
}
#[derive(Clone)]
pub(crate) struct Source {
    name: String,
    protocol: Protocol,
    url: String,
    ck: Option<u32>,     // 32-bit CRC 
    md5: Option<[u8; 16]>,   // 128-bit MD5
    sha1: Option<[u8; 20]>,  // 160-bit SHA-1
    sha224: Option<[u8; 28]>,// 224-bit SHA-2
    sha256: Option<[u8; 32]>,// 256-bit SHA-2
    sha384: Option<[u8; 48]>,// 384-bit SHA-2
    sha512: Option<[u8; 64]>,// 512-bit SHA-2
    b2: Option<[u8; 64]>,    // 512-bit Blake-2B
}

struct SourceCache {
    path: PathBuf,
    url: String,
}

fn push_source(
    sources: &mut Vec<Source>, 
    name: Option<String>, 
    protocol: Option<Protocol>,
    url: Option<String>,
    ck: Option<u32>,     // 32-bit CRC 
    md5: Option<[u8; 16]>,   // 128-bit MD5
    sha1: Option<[u8; 20]>,  // 160-bit SHA-1
    sha224: Option<[u8; 28]>,// 224-bit SHA-2
    sha256: Option<[u8; 32]>,// 256-bit SHA-2
    sha384: Option<[u8; 48]>,// 384-bit SHA-2
    sha512: Option<[u8; 64]>,// 512-bit SHA-2
    b2: Option<[u8; 64]>,    // 512-bit Blake-2B
) {
    if let Some(name) = name {
        if let Some(protocol) = protocol {
            if let Some(url) = url {
                sources.push(Source{
                    name,
                    protocol,
                    url,
                    ck,
                    md5,
                    sha1,
                    sha224,
                    sha256,
                    sha384,
                    sha512,
                    b2,
                });
                return
            }
        }
    };
    panic!("Unfinished source definition")
}

pub(crate) fn get_sources<P> (pkgbuild: &Path) -> Vec<Source>
where
    P: AsRef<Path>
{
    const SCRIPT: &str = include_str!("scripts/get_sources.bash");
    let output = Command::new("/bin/bash")
        .arg("-c")
        .arg(SCRIPT)
        .arg("Source reader")
        .arg(pkgbuild)
        .output()
        .expect("Failed to run script");
    let mut name = None;
    let mut protocol = None;
    let mut url = None;
    let mut ck = None;
    let mut md5 = None;
    let mut sha1 = None;
    let mut sha224 = None;
    let mut sha256 = None;
    let mut sha384 = None;
    let mut sha512 = None;
    let mut b2 = None;
    let mut sources = vec![];
    // let source = sources.last();
    let mut started = false;
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        if line == "[source]" {
            if started {
                push_source(&mut sources, 
                    name, protocol, url, 
                    ck, md5, sha1, 
                    sha224, sha256, sha384, sha512, 
                    b2);
                name = None;
                protocol = None;
                url = None;
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
        } else {
            let mut it = line.splitn(2, ": ");
            let key = it.next().expect("Failed to get key");
            let value = it.next().expect("Failed to get value");
            match key {
                "name" => {
                    name = Some(value.to_string().clone());
                }
                "protocol" => {
                    protocol = Some(Protocol::from_string(value));
                }
                "url" => {
                    url = Some(value.to_string().clone());
                }
                "cksum" => {
                    ck = Some(value.parse().expect("Failed to parse 32-bit CRC"));
                    println!("CRC checksum: {}", value);
                }
                "md5sum" => {
                    md5 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 128-bit MD5 sum"));
                }
                "sha1sum" => {
                    sha1 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 160-bit SHA-1 sum"));
                }
                "sha224sum" => {
                    sha224 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 224-bit SHA-2 sum"));
                }
                "sha256sum" => {
                    sha256 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 256-bit SHA-2 sum"));
                }
                "sha384sum" => {
                    sha384 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 384-bit SHA-2 sum"));
                }
                "sha512sum" => {
                    sha512 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 512-bit SHA-2 sum"));
                }
                "b2sum" => {
                    b2 = Some(FromHex::from_hex(value)
                        .expect("Failed to parse 512-bit Blake-2B sum"));
                }
                &_ => {
                    println!("Other thing: {}", line);
                    panic!("Unexpected line");
                }
            }
        }
    }
    push_source(&mut sources, 
        name, protocol, url, 
        ck, md5, sha1, 
        sha224, sha256, sha384, sha512, 
        b2);
    sources
}

fn update_unique_sources(unique_sources: &mut Vec<Source>, source: &Source) {
    let mut existing = None;
    for unique_source in unique_sources.iter_mut() {
        if cksums::optional_equal(&unique_source.ck, &source.ck) ||
           cksums::optional_equal(&unique_source.md5, &source.md5) ||
           cksums::optional_equal(&unique_source.sha1, &source.sha1) ||
           cksums::optional_equal(&unique_source.sha224, &source.sha224) ||
           cksums::optional_equal(&unique_source.sha256, &source.sha256) ||
           cksums::optional_equal(&unique_source.sha384, &source.sha384) ||
           cksums::optional_equal(&unique_source.sha512, &source.sha512) ||
           cksums::optional_equal(&unique_source.b2, &source.b2) {
            existing = Some(unique_source);
            break;
        }
    }
    let unique_source = match existing {
        Some(unique_source) => unique_source,
        None => {
            unique_sources.push(source.clone());
            unique_sources.last_mut().expect("Failed to get unique source we just added")
        },
    };
    cksums::optional_update(&mut unique_source.ck, &source.ck);
    cksums::optional_update(&mut unique_source.md5, &source.md5);
    cksums::optional_update(&mut unique_source.sha1, &source.sha1);
    cksums::optional_update(&mut unique_source.sha224, &source.sha224);
    cksums::optional_update(&mut unique_source.sha256, &source.sha256);
    cksums::optional_update(&mut unique_source.sha384, &source.sha384);
    cksums::optional_update(&mut unique_source.sha512, &source.sha512);
    cksums::optional_update(&mut unique_source.b2, &source.b2);

}

pub(crate) fn dedup_sources(sources: &Vec<Source>) -> Vec<Source> {
    let mut unique_sources: Vec<Source> = vec![];
    for source in sources.iter() {
        if let Protocol::Local = source.protocol {
            continue;
        }
        update_unique_sources(&mut unique_sources, source);
    }
    unique_sources
}

fn print_source(source: &Source) {
    println!("Source '{}' from '{}' protocol '{:?}'", source.name, source.url, source.protocol);
    if let Some(ck) = source.ck {
        println!("=> CKSUM: {:x}", ck);
    }
    if let Some(md5) = source.md5 {
        println!("=> md5sum: {}", cksums::print(&md5));
    }
    if let Some(sha1) = source.sha1 {
        println!("=> sha1sum: {}", cksums::print(&sha1));
    }
    if let Some(sha224) = source.sha224 {
        println!("=> sha224sum: {}", cksums::print(&sha224));
    }
    if let Some(sha256) = source.sha256 {
        println!("=> sha256sum: {}", cksums::print(&sha256));
    }
    if let Some(sha384) = source.sha384 {
        println!("=> sha384sum: {}", cksums::print(&sha384));
    }
    if let Some(sha512) = source.sha512 {
        println!("=> sha512sum: {}", cksums::print(&sha512));
    }
    if let Some(b2) = source.b2 {
        println!("=> b2sum: {}", cksums::print(&b2));
    }
}


pub(crate) fn cache_sources(sources: &Vec<Source>) {
    for source in sources.iter() {
        print_source(source);
    }
    return;

    let mut gits = vec![];
    let git_parent = PathBuf::from("sources/git");
    for source in sources.iter() {
        match &source.protocol {
            Protocol::Netfile { protocol } => {
                match protocol {
                    NetfileProtocol::File => todo!(),
                    NetfileProtocol::Ftp => todo!(),
                    NetfileProtocol::Http => todo!(),
                    NetfileProtocol::Https => todo!(),
                    NetfileProtocol::Rsync => todo!(),
                    NetfileProtocol::Scp => todo!(),
                }
            },
            Protocol::Vcs { protocol } => {
                match protocol {
                    VcsProtocol::Bzr => todo!(),
                    VcsProtocol::Fossil => todo!(),
                    VcsProtocol::Git => gits.push(SourceCache {
                        path: git_parent.join(format!("{:016x}", xxh3_64(source.url.as_bytes()))),
                        url: source.url.clone(),
                    }),
                    VcsProtocol::Hg => todo!(),
                    VcsProtocol::Svn => todo!(),
                }
            },
            Protocol::Local => todo!(),
        }
        // match source

    }
}