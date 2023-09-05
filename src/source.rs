use std::{path::Path, process::Command};

use blake2::Blake2b512;
use crc::Crc;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};


#[derive(Debug)]
enum NetfileProtocol {
    File,
    Ftp,
    Http,
    Https,
    Rsync,
    Scp,
}

#[derive(Debug)]
enum VcsProtocol {
    Bzr,
    Fossil,
    Git,
    Hg,
    Svn,
}

#[derive(Debug)]
enum Protocol {
    Netfile {
        protocol: NetfileProtocol
    },
    Vcs {
        protocol: VcsProtocol
    },
    Local
}

struct Source {
    name: String,
    protocol: Protocol,
    url: String,
    // ck: Option<Crc<u32>>,
    // md5: Option<md5::Digest>,
    // sha1: Option<Sha1>,
    // sha224: Option<Sha224>,
    // sha256: Option<Sha256>,
    // sha384: Option<Sha384>,
    // sha512: Option<Sha512>,
    // b2: Option<Blake2b512>
}

fn push_source(
    sources: &mut Vec<Source>, 
    name: Option<String>, 
    protocol: Option<Protocol>,
    url: Option<String>
) {
    if let Some(name) = name {
        if let Some(protocol) = protocol {
            if let Some(url) = url {
                sources.push(Source{
                    name,
                    protocol,
                    url,
                    // ck,
                    // md5,
                    // sha1,
                    // sha224,
                    // sha256,
                    // sha384,
                    // sha512,
                    // b2,
                });
                return
            }
        }
    };
    panic!("Unfinished source definition")
}

pub(crate) fn get_sources<P> (pkgbuild: &Path) 
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
    // let mut ck = None;
    // let mut md5 = None;
    // let mut sha1 = None;
    // let mut sha224 = None;
    // let mut sha256 = None;
    // let mut sha384 = None;
    // let mut sha512 = None;
    // let mut b2 = None;
    let mut sources = vec![];
    // let source = sources.last();
    let mut started = false;
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        if line == "[source]" {
            if started {
                push_source(&mut sources, name, protocol, url);
                name = None;
                protocol = None;
                url = None;
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
                    protocol = Some(match value {
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
                    });
                }
                "url" => {
                    url = Some(value.to_string().clone());
                }
                "cksum" => {
                    println!("CRC checksum: {}", value);
                }
                "md5sum" => {
                    println!("MD5 checksum: {}", value);
                }
                "sha1sum" => {
                    println!("SHA1 checksum: {}", value);
                }
                "sha224sum" => {
                    println!("SHA224 checksum: {}", value);
                }
                "sha256sum" => {
                    println!("SHA256 checksum: {}", value);
                }
                "sha384sum" => {
                    println!("SHA384 checksum: {}", value);
                }
                "sha512sum" => {
                    println!("SHA512 checksum: {}", value);
                }
                "b2sum" => {
                    println!("B2 checksum: {}", value);
                }
                &_ => {
                    println!("Other thing: {}", line);
                    panic!("Unexpected line");
                }
            }
        }
    }
    push_source(&mut sources, name, protocol, url);
    for source in sources.iter() {
        println!("Source {} from {}, protocol {:?}", source.name, source.url, source.protocol);
    }
}
