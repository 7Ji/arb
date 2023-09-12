use std::{path::{Path, PathBuf}, process::Command, fmt::Display, collections::HashMap, str::FromStr, ops::Deref, thread::{self, JoinHandle, sleep}, time::Duration};
use hex::FromHex;
use xxhash_rust::xxh3::xxh3_64;

use crate::{cksums, git};


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
    hash_url: u64,
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
    hash_url: u64,
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
    // let source = sources.last();
    let mut started = false;
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        if line == "[source]" {
            if started {
                push_source(&mut sources, 
                    name, protocol, url, hash_url,
                    ck, md5, sha1, 
                    sha224, sha256, sha384, sha512, 
                    b2);
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
        } else {
            let mut it = line.splitn(2, ": ");
            let key = it.next().expect("Failed to get key");
            let value = it.next().expect("Failed to get value");
            match key {
                "name" => {
                    name = Some(value.to_string());
                }
                "protocol" => {
                    protocol = Some(Protocol::from_string(value));
                }
                "url" => {
                    let url_string = value.to_string();
                    hash_url = xxh3_64(url_string.as_bytes());
                    url = Some(url_string);
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
        name, protocol, url, hash_url,
        ck, md5, sha1, 
        sha224, sha256, sha384, sha512, 
        b2);
    sources
}

fn push_netfile_sources(netfile_sources: &mut Vec<Source>, source: &Source) {
    let mut existing = None;
    for netfile_source in netfile_sources.iter_mut() {
        if cksums::optional_equal(&netfile_source.ck, &source.ck) ||
           cksums::optional_equal(&netfile_source.md5, &source.md5) ||
           cksums::optional_equal(&netfile_source.sha1, &source.sha1) ||
           cksums::optional_equal(&netfile_source.sha224, &source.sha224) ||
           cksums::optional_equal(&netfile_source.sha256, &source.sha256) ||
           cksums::optional_equal(&netfile_source.sha384, &source.sha384) ||
           cksums::optional_equal(&netfile_source.sha512, &source.sha512) ||
           cksums::optional_equal(&netfile_source.b2, &source.b2) {
            existing = Some(netfile_source);
            break;
        }
    }
    let netfile_source = match existing {
        Some(netfile_source) => netfile_source,
        None => {
            netfile_sources.push(source.clone());
            netfile_sources.last_mut().expect("Failed to get unique source we just added")
        },
    };
    cksums::optional_update(&mut netfile_source.ck, &source.ck);
    cksums::optional_update(&mut netfile_source.md5, &source.md5);
    cksums::optional_update(&mut netfile_source.sha1, &source.sha1);
    cksums::optional_update(&mut netfile_source.sha224, &source.sha224);
    cksums::optional_update(&mut netfile_source.sha256, &source.sha256);
    cksums::optional_update(&mut netfile_source.sha384, &source.sha384);
    cksums::optional_update(&mut netfile_source.sha512, &source.sha512);
    cksums::optional_update(&mut netfile_source.b2, &source.b2);
}

fn push_git_sources(git_sources: &mut Vec<Source>, source: &Source) {
    for git_source in git_sources.iter() {
        if git_source.hash_url == source.hash_url {
            return
        }
    }
    git_sources.push(source.clone())
}

pub(crate) fn unique_sources(sources: &Vec<&Source>) -> (Vec<Source>, Vec<Source>, Vec<Source>) {
    let mut local_sources: Vec<Source> = vec![];
    let mut git_sources: Vec<Source> = vec![];
    let mut netfile_sources: Vec<Source> = vec![];
    for source in sources.iter() {
        match &source.protocol {
            Protocol::Netfile { protocol: _ } => push_netfile_sources(&mut netfile_sources, source),
            Protocol::Vcs { protocol } => {
                match protocol {  // Ignore VCS sources we do not support
                    VcsProtocol::Bzr => (),
                    VcsProtocol::Fossil => (),
                    VcsProtocol::Git => push_git_sources(&mut git_sources, source),
                    VcsProtocol::Hg => (),
                    VcsProtocol::Svn => (),
                }
            },
            Protocol::Local => local_sources.push(source.to_owned().to_owned())
        }
    }
    (netfile_sources, git_sources, local_sources)
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

fn cache_netfile_source(protocol: &NetfileProtocol, source: &Source) {
    let mut http_sources = vec![];
    let mut file_sources = vec![];
    let mut ftp_sources = vec![];
    let mut rsync_sources = vec![];
    let mut scp_sources = vec![];
    match protocol {
        NetfileProtocol::File => file_sources.push(source),
        NetfileProtocol::Ftp => ftp_sources.push(source),
        NetfileProtocol::Http => http_sources.push(source),
        NetfileProtocol::Https => http_sources.push(source),
        NetfileProtocol::Rsync => rsync_sources.push(source),
        NetfileProtocol::Scp => scp_sources.push(source),
    }
}

fn cache_vcs_source(protocol: &VcsProtocol, source: &Source) {
    // match protocol {
    //     VcsProtocol::Bzr => (),
    //     VcsProtocol::Fossil => (),
    //     VcsProtocol::Git => git::sync_repo(
            
    //         // GIT_PARENT.clone().join(format!("{:016x}", xxh3_64(source.url.as_bytes()))), &source.url, None),
    //     VcsProtocol::Hg => (),
    //     VcsProtocol::Svn => (),
    // }
}

fn cache_sources(sources: &Vec<Source>) {
    for source in sources.iter() {
        match &source.protocol {
            Protocol::Netfile { protocol } => cache_netfile_source(protocol, source),
            Protocol::Vcs { protocol } => cache_vcs_source(protocol, source),
            Protocol::Local => (),
        }
    }
}

fn cache_netfile_sources_mt(netfile_sources: HashMap<u64, Vec<Source>>, proxy: Option<&str>) {
    println!("Caching netfile sources with {} threads", netfile_sources.len());
    
    // let mut map = HashMap::new();
    // for source in sources.iter() {
    //     let url = url::Url::from_str(&source.url)
    //         .expect("Failed to parse URL");
    //     let domain = xxh3_64(url.domain().expect("Failed to get domain").as_bytes());
    //     if ! map.contains_key(&domain) {
    //         println!("New domain found from source URL '{}'", source.url);
    //         map.insert(domain, vec![]);
    //     }
    //     let vec = map
    //         .get_mut(&domain)
    //         .expect("Failed to get vec");
    //     vec.push(source);
    // }
    // println!("Domains count for sources: {}", map.len());
    // let mut threads = vec![];
    // for (_, sources) in map {
    //     let sources_thread: Vec<Source> = 
    //         sources.into_iter().map(
    //             |source| source.clone()).collect();
    //     threads.push(std::thread::spawn(move|| {
    //         cache_sources(&sources_thread)
    //     }))
    // }
    // for thread in threads {
    //     thread.join().expect("Failed to join thread");
    // }
    // println!("Finished");
}

fn cache_git_sources_for_domain_mt(git_sources: Vec<Source>, proxy: Option<&str>) {
    const REFSPECS: &[&str] = &[
        "+refs/heads/*:refs/heads/*", 
        "+refs/tags/*:refs/tags/*"
    ];
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads: Vec<JoinHandle<()>> = vec![];
    for git_source in git_sources {
        let mut thread_id_finished = None;
        if threads.len() > 3 {
            while let None = thread_id_finished {
                for (thread_id, thread) in threads.iter().enumerate() {
                    if thread.is_finished() {
                        thread_id_finished = Some(thread_id);
                    }
                }
                sleep(Duration::from_secs(1));
            }
            if let Some(thread_id_finished) = thread_id_finished {
                threads.swap_remove(thread_id_finished).join().expect("Failed to join finished thread");
            } else {
                panic!("Failed to get finished thread ID")
            }
        }
        let proxy_string_thread = proxy_string.clone();
        let path = PathBuf::from(format!("sources/git/{:016x}", xxh3_64(git_source.url.as_bytes())));
        threads.push(thread::spawn(move ||{
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            git::sync_repo(&path, &git_source.url, proxy, REFSPECS)}
        ));
    }
    for thread in threads.into_iter() {
        thread.join().expect("Failed to join finished thread");
    }
}

fn cache_git_sources_mt(git_sources: HashMap<u64, Vec<Source>>, proxy: Option<&str>) {
    println!("Caching git sources with {} groups", git_sources.len());
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads: Vec<std::thread::JoinHandle<()>> =  Vec::new();
    for git_sources in git_sources.into_values() {
        let proxy_string_thread = proxy_string.clone();
        threads.push(thread::spawn(move || {
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            cache_git_sources_for_domain_mt(git_sources, proxy);
        }));
    }
    for thread in threads {
        thread.join().expect("Failed to join git cacher threads");
    }
}

fn map_sources_by_domain(sources: &Vec<Source>) -> HashMap<u64, Vec<Source>> {
    let mut map = HashMap::new();
    for source in sources.iter() {
        let url = url::Url::from_str(&source.url)
            .expect("Failed to parse URL");
        let domain = xxh3_64(url.domain().expect("Failed to get domain").as_bytes());
        if ! map.contains_key(&domain) {
            map.insert(domain, vec![]);
        }
        let vec = map
            .get_mut(&domain)
            .expect("Failed to get vec");
        vec.push(source.to_owned());
    }
    map
}

pub(crate) fn cache_sources_mt(netfile_sources: &Vec<Source>, git_sources: &Vec<Source>, proxy: Option<&str>) {
    let netfile_sources_map = map_sources_by_domain(netfile_sources);
    let git_sources_map = map_sources_by_domain(git_sources);
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let proxy_string_dup = proxy_string.clone();
    let netfile_thread = std::thread::spawn(move || {
        let proxy = match has_proxy {
            true => Some(proxy_string_dup.as_str()),
            false => None,
        };
        cache_netfile_sources_mt(netfile_sources_map, proxy)
    });
    let git_thread = std::thread::spawn(move || {
        let proxy = match has_proxy {
            true => Some(proxy_string.as_str()),
            false => None,
        };
        cache_git_sources_mt(git_sources_map, proxy)
    });
    // let git_thread = std::thread::spawn(move || cache_git_sources_mt(git_sources_map, proxy));
    netfile_thread.join().expect("Failed to join netfile thread");
    git_thread.join().expect("Failed to join git thread");
    println!("Finished multi-threading caching sources");
}