use std::{path::Path, process::Command};
use hex::FromHex;


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

struct Source {
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
    for source in sources.iter() {
        println!("Source {} from {}, protocol {:?}", source.name, source.url, source.protocol);
    }
}
