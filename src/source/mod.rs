use crate::threading;
use std::{
        collections::HashMap,
        fs::{
            read_dir,
            remove_dir_all,
            remove_file,
        },
        os::unix::fs::symlink,
        path::{
            Path,
            PathBuf,
        },
        str::FromStr,
        thread::{
            self,
            JoinHandle,
        }
    };
use xxhash_rust::xxh3::xxh3_64;

mod cksums;
mod download;
pub(crate) mod git;
mod protocol;
mod netfile;
mod parse;

use cksums::{
    IntegFile,
    Cksum,
    Md5sum,
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
    B2sum,
};

use git::ToReposMap;

use protocol::{
    Protocol,
    VcsProtocol,
};

pub(crate) use parse::{
    get_sources,
    unique_sources
};

#[derive(Clone)]
pub(crate) struct Source {
    name: String,
    protocol: Protocol,
    url: String,
    hash_url: u64,
    ck: Option<Cksum>,     // 32-bit CRC
    md5: Option<Md5sum>,   // 128-bit MD5
    sha1: Option<Sha1sum>,  // 160-bit SHA-1
    sha224: Option<Sha224sum>,// 224-bit SHA-2
    sha256: Option<Sha256sum>,// 256-bit SHA-2
    sha384: Option<Sha384sum>,// 384-bit SHA-2
    sha512: Option<Sha512sum>,// 512-bit SHA-2
    b2: Option<B2sum>,    // 512-bit Blake-2B
}

pub(crate) trait MapByDomain {
    fn url(&self) -> &str;
    fn map_by_domain(sources: &Vec<Self>) -> HashMap<u64, Vec<Self>>
    where
        Self: Clone + Sized
    {
        let mut map = HashMap::new();
        for source in sources.iter() {
            let url =
                url::Url::from_str(source.url())
                .expect("Failed to parse URL");
            let domain = xxh3_64(
                url.domain().expect("Failed to get domain")
                .as_bytes());
            if ! map.contains_key(&domain) {
                map.insert(domain, vec![]);
            }
            let vec = map
                .get_mut(&domain)
                .expect("Failed to get vec");
            vec.push(source.clone());
        }
        map
    }
}

impl MapByDomain for Source {
    fn url(&self) -> &str {
        self.url.as_str()
    }
}

fn optional_equal<C:PartialEq>(a: &Option<C>, b: &Option<C>)
    -> bool
{
    if let Some(a) = a {
        if let Some(b) = b {
            if a == b {
                return true
            }
        }
    }
    false
}

fn optional_update<C>(target: &mut Option<C>, source: &Option<C>)
-> Result<(), ()>
    where C: PartialEq + Clone 
{
    if let Some(target) = target {
        if let Some(source) = source {
            if target != source {
                eprintln!("Source target mismatch");
                return Err(());
            }
        }
    } else if let Some(source) = source {
        *target = Some(source.clone())
    }
    Ok(())
}

fn get_domain_threads_map<T>(orig_map: &HashMap<u64, Vec<T>>) 
    -> Option<HashMap<u64, Vec<JoinHandle<Result<(), ()>>>>>
{
    let mut map = HashMap::new();
    for key in orig_map.keys() {
        match map.insert(*key, vec![]) {
            Some(_) => {
                eprintln!("Duplicated domain for thread: {:x}", key);
                return None
            },
            None => (),
        }
    }
    Some(map)
}

fn get_domain_threads_from_map<'a>(
    domain: &u64, 
    map: &'a mut HashMap<u64, Vec<JoinHandle<Result<(), ()>>>>
) -> Option<&'a mut Vec<JoinHandle<Result<(), ()>>>>
{
    match map.get_mut(domain) {
        Some(threads) => Some(threads),
        None => {
            println!(
                "Domain {:x} has no threads, which should not happen", domain);
            None
        },
    }
}

pub(crate) fn cache_sources_mt(
    netfile_sources: &Vec<Source>,
    git_sources: &Vec<Source>,
    holdgit: bool,
    skipint: bool,
    proxy: Option<&str>,
    gmr: Option<&git::Gmr>
) -> Result<(), ()> 
{
    netfile::ensure_parents()?;
    let mut netfile_sources_map =
        Source::map_by_domain(netfile_sources);
    let git_sources_map =
        Source::map_by_domain(git_sources);
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut netfile_threads_map = 
        match get_domain_threads_map(&netfile_sources_map) {
            Some(map) => map,
            None => {
                eprintln!("Failed to get netfile threads map");
                return Err(())
            },
        };
    let mut git_threads_map = 
        match get_domain_threads_map(&git_sources_map) {
            Some(map) => map,
            None => {
                eprintln!("Failed to get git threads map");
                return Err(())
            },
        };
    let mut git_repos_map = 
        match Source::to_repos_map(git_sources_map, "sources/git", gmr) {
            Some(git_repos_map) => git_repos_map,
            None => {
                eprintln!("Failed to get git repos map");
                return Err(())
            },
        };
    const MAX_THREADS: usize = 10;
    let mut bad = false;
    while netfile_sources_map.len() > 0 || git_repos_map.len() > 0 {
        for (domain, netfile_sources) in 
            netfile_sources_map.iter_mut() 
        {
            let netfile_threads = match
                get_domain_threads_from_map(domain, &mut netfile_threads_map) 
            {
                Some(threads) => threads,
                None => return Err(()),
            };
            while netfile_sources.len() > 0 && 
                netfile_threads.len() < MAX_THREADS 
            {
                let netfile_source = netfile_sources
                    .pop()
                    .expect("Failed to get source from sources vec");
                let integ_files 
                    = IntegFile::vec_from_source(&netfile_source);
                let proxy_string_thread = proxy_string.clone();
                let netfile_thread = thread::spawn(
                move ||{
                    let proxy = match has_proxy {
                        true => Some(proxy_string_thread.as_str()),
                        false => None,
                    };
                    netfile::cache_source(
                        &netfile_source, &integ_files, skipint, proxy)
                });
                netfile_threads.push(netfile_thread);
            }
        }
        for (domain, git_repos) in 
            git_repos_map.iter_mut() 
        {
            let git_threads = match
                get_domain_threads_from_map(domain, &mut git_threads_map) 
            {
                Some(threads) => threads,
                None => return Err(()),
            };
            while git_repos.len() > 0 && 
                git_threads.len() < MAX_THREADS 
            {
                let git_repo = git_repos
                    .pop()
                    .expect("Failed to get source from sources vec");
                if holdgit && git_repo.healthy() {
                    continue
                }
                let proxy_string_thread = proxy_string.clone();
                let git_thread = thread::spawn(
                move ||{
                    let proxy = match has_proxy {
                        true => Some(proxy_string_thread.as_str()),
                        false => None,
                    };
                    git_repo.sync(proxy, git::Refspecs::HeadsTags)
                });
                git_threads.push(git_thread);
            }
        }
        if let Err(_) = threading::wait_thread_map(
            &mut netfile_threads_map, "caching netfile sources") {
                bad = true
            }
        if let Err(_) = threading::wait_thread_map(
            &mut git_threads_map, "caching git sources") {
                bad = true
            }
        netfile_sources_map.retain(
            |_, sources| sources.len() > 0);
        git_repos_map.retain(
            |_, repos| repos.len() > 0);
    }
    let mut remaining_threads = vec![];
    for mut threads in 
        netfile_threads_map.into_values() 
    {
        remaining_threads.append(&mut threads);
    }
    for mut threads in 
        git_threads_map.into_values() 
    {
        remaining_threads.append(&mut threads);
    }
    match threading::wait_remaining(remaining_threads, "caching sources") {
        Ok(_) => (),
        Err(_) => bad = true,
    }
    println!("Finished multi-threading caching sources");
    if bad {
        Err(())
    } else {
        Ok(())
    }
}

pub(crate) fn extract<P: AsRef<Path>>(dir: P, sources: &Vec<Source>) {
    let rel = PathBuf::from("../..");
    for source in sources.iter() {
        let mut original = None;
        match &source.protocol {
            Protocol::Netfile { protocol: _ } => {
                let integ_files = IntegFile::vec_from_source(source);
                if let Some(integ_file) = integ_files.last() {
                    original = Some(rel.join(integ_file.get_path()));
                }
            },
            Protocol::Vcs { protocol } =>
                if let VcsProtocol::Git = protocol {
                    original = Some(rel
                        .join(format!("sources/git/{:016x}",
                                xxh3_64(source.url.as_bytes()))));
                },
            Protocol::Local => (),
        }
        if let Some(original) = original {
            symlink(original,
                dir.as_ref().join(&source.name))
                .expect("Failed to symlink")
        }
    }
}

// Used must be already sorted
pub(crate) fn remove_unused<P: AsRef<Path>>(dir: P, used: &Vec<String>) {
    let readdir = match read_dir(dir) {
        Ok(readdir) => readdir,
        Err(_) => return,
    };
    for entry in readdir {
        match entry {
            Ok(entry) => {
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                let name =
                    entry.file_name().to_string_lossy().into_owned();
                match used.binary_search(&name) {
                    Ok(_) => continue,
                    Err(_) => (),
                }
                if metadata.is_dir() {
                    println!("Removing '{}' not used any more",
                        entry.path().display());
                    let _ = remove_dir_all(entry.path());
                }
                if metadata.is_file() || metadata.is_symlink() {
                    println!("Removing '{}' not used any more",
                        entry.path().display());
                    let _ = remove_file(entry.path());
                }
            },
            Err(_) => return,
        }
    }
}

fn clean_netfile_sources(sources: &Vec<Source>) -> Vec<JoinHandle<()>>{
    let mut ck_used = vec![];
    let mut md5_used = vec![];
    let mut sha1_used = vec![];
    let mut sha224_used = vec![];
    let mut sha256_used = vec![];
    let mut sha384_used = vec![];
    let mut sha512_used = vec![];
    let mut b2_used = vec![];
    let mut cleaners = vec![];
    for source in sources.iter() {
        if let Some(ck) = &source.ck {
            ck_used.push(ck.to_string());
        }
        if let Some(md5) = &source.md5 {
            md5_used.push(md5.to_string());
        }
        if let Some(sha1) = &source.sha1 {
            sha1_used.push(sha1.to_string());
        }
        if let Some(sha224) = &source.sha224 {
            sha224_used.push(sha224.to_string());
        }
        if let Some(sha256) = &source.sha256 {
            sha256_used.push(sha256.to_string());
        }
        if let Some(sha384) = &source.sha384 {
            sha384_used.push(sha384.to_string());
        }
        if let Some(sha512) = &source.sha512 {
            sha512_used.push(sha512.to_string());
        }
        if let Some(b2) = &source.b2 {
            b2_used.push(b2.to_string());
        }
    }
    ck_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-ck", &ck_used)));
    md5_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-md5", &md5_used)));
    sha1_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha1", &sha1_used)));
    sha224_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha224", &sha224_used)));
    sha256_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha256", &sha256_used)));
    sha384_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha384", &sha384_used)));
    sha512_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha512", &sha512_used)));
    b2_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-b2", &b2_used)));
    cleaners
}

fn clean_git_sources(sources: &Vec<Source>) {
    let hashes: Vec<u64> = sources.iter().map(
        |source| xxh3_64(source.url.as_bytes())).collect();
    let mut used: Vec<String> = hashes.iter().map(
        |hash| format!("{:016x}", hash)).collect();
    used.sort_unstable();
    remove_unused("sources/git", &used);
}

pub(crate) fn cleanup(netfile_sources: Vec<Source>, git_sources: Vec<Source>)
    -> Vec<JoinHandle<()>>
{
    let mut cleaners =
        clean_netfile_sources(&netfile_sources);
    cleaners.push(thread::spawn(move||clean_git_sources(&git_sources)));
    cleaners
}