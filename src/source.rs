use std::{path::Path, process::Command};

use blake2::Blake2b512;
use crc::{CRC_32_CKSUM, Crc};
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512};

enum NetfileProtocol {
    File,
    Ftp,
    Http,
    Https,
    Rsync,
    Scp,
    Unknown
}

enum VcsProtocol {
    Bzr,
    Fossil,
    Git,
    Hg,
    Svn
}

enum Protocol {
    Netfile {
        Protocol: NetfileProtocol
    },
    Vcs {
        Protocol: VcsProtocol
    },
}

struct Source {
    name: String,
    protocol: Protocol,
    url: String,
    cksum: Option<Crc<u32>>,
    md5: Option<md5::Digest>,
    sha1: Option<Sha1>,
    sha224: Option<Sha224>,
    sha256: Option<Sha256>,
    sha384: Option<Sha384>,
    sha512: Option<Sha512>,
    b2: Option<Blake2b512>
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
    let raw = String::from_utf8_lossy(&output.stdout);
    let mut protocol: Protocol;
    protocol = Protocol::Netfile { Protocol: NetfileProtocol::File };
    for line in raw.lines() {
        if line == "[source]" {
            println!("Start definition");
        } else {
            let mut it = line.splitn(2, ": ");
            let key = it.next().expect("Failed to get key");
            let value = it.next().expect("Failed to get value");
            match key {
                "name" => {
                    println!("Name: {}", value);
                }
                "protocol" => {
                    println!("Protocol: {}", value);
                }
                "url" => {
                    println!("URL: {}", value);
                }
                "cksum" => {
                    println!("CRC checksum: {}", value);
                }
                "md5sum" => {
                    // md5
                    
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
}
