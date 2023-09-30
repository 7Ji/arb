
use std::{
        collections::HashMap,
        fs::DirBuilder,
        thread::{
            self,
            JoinHandle,
        },
    };

use super::{
        Source,
        protocol::{
            NetfileProtocol,
            Protocol,
        },
        download,
        git::ToReposMap,
    };

pub(super) fn ensure_parents() -> Result<(), ()>
{
    let mut dir_builder = DirBuilder::new();
    dir_builder.recursive(true);
    for integ in
        ["ck", "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "b2"]
    {
        let folder = format!("sources/file-{}", integ);
        match dir_builder.create(&folder) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to create folder '{}': {}", &folder, e);
                return Err(())
            },
        }
    }
    Ok(())
}

fn optional_equal<C:PartialEq + std::fmt::Display>(a: &Option<C>, b: &Option<C>)
    -> bool
{
    if let Some(a) = a {
        if let Some(b) = b {
            if a == b {
                println!("Duplicated integrity checksum: '{}' == ''{}'", a, b);
                return true
            }
        }
    }
    false
}

fn optional_update<C>(target: &mut Option<C>, source: &Option<C>)
-> Result<(), ()>
    where C: PartialEq + Clone + std::fmt::Display
{
    if let Some(target) = target {
        if let Some(source) = source {
            if target != source {
                eprintln!("Source target mismatch {} != {}", source, target);
                return Err(());
            }
        }
    } else if let Some(source) = source {
        *target = Some(source.clone())
    }
    Ok(())
}

pub(super) fn push_source(
    sources: &mut Vec<Source>, source: &Source
) -> Result<(), ()> 
{
    let mut existing = None;
    for source in sources.iter_mut() {
        if optional_equal(
                &source.ck, &source.ck) ||
           optional_equal(
                &source.md5, &source.md5) ||
           optional_equal(
                &source.sha1, &source.sha1) ||
           optional_equal(
                &source.sha224, &source.sha224) ||
           optional_equal(
                &source.sha256, &source.sha256) ||
           optional_equal(
                &source.sha384, &source.sha384) ||
           optional_equal(
                &source.sha512, &source.sha512) ||
           optional_equal(&source.b2, &source.b2) {
            existing = Some(source);
            break;
        }
    }
    let existing = match existing {
        Some(source) => source,
        None => {
            sources.push(source.clone());
            return Ok(())
        },
    };
    optional_update(
        &mut existing.ck, &source.ck)?;
    optional_update(
        &mut existing.md5, &source.md5)?;
    optional_update(
        &mut existing.sha1, &source.sha1)?;
    optional_update(
        &mut existing.sha224, &source.sha224)?;
    optional_update(
        &mut existing.sha256, &source.sha256)?;
    optional_update(
        &mut existing.sha384, &source.sha384)?;
    optional_update(
        &mut existing.sha512, &source.sha512)?;
    optional_update(
        &mut existing.b2, &source.b2)
}

pub(super) fn download_source(
    source: &Source,
    integ_file: &super::cksums::IntegFile,
    skipint: bool,
    proxy: Option<&str>
) -> Result<(), ()> 
{
    let protocol = 
        if let Protocol::Netfile{protocol} = &source.protocol {
            protocol
        } else {
            eprintln!("Non-netfile source encountered by netfile cacher");
            return Err(())
        };
    let url = source.url.as_str();
    let path = integ_file.get_path();
    for _ in 0..2 {
        println!("Downloading '{}' to '{}'",
            source.url, path.display());
        if let Ok(_) = match &protocol {
            NetfileProtocol::File => download::file(url, path),
            NetfileProtocol::Ftp => download::ftp(url, path),
            NetfileProtocol::Http => 
                download::http(url, path, None),
            NetfileProtocol::Https => 
                download::http(url, path, None),
            NetfileProtocol::Rsync => download::rsync(url, path),
            NetfileProtocol::Scp => download::scp(url, path),
        } {
            if integ_file.valid(skipint) {
                return Ok(())
            }
        }
    }
    if let None = proxy {
        eprintln!(
            "Failed to download netfile source '{}' and no proxy to retry", 
            source.url);
        return Err(())
    }
    if match &protocol {
        NetfileProtocol::File => false,
        NetfileProtocol::Ftp => false,
        NetfileProtocol::Http => true,
        NetfileProtocol::Https => true,
        NetfileProtocol::Rsync => false,
        NetfileProtocol::Scp => false,
    } {
        println!("Failed to download '{}' to '{}' after 3 tries, use proxy",
                source.url, path.display());
    } else {
        eprintln!(
            "Failed to download netfile source '{}', proto not support proxy", 
            source.url);
        return Err(())
    }
    for _  in 0..2 {
        println!("Downloading '{}' to '{}'",
                source.url, path.display());
        if let Ok(_) = download::http(url, path, proxy) {
            if integ_file.valid(skipint) {
                return Ok(())
            }
        }
    }
    eprintln!("Failed to download netfile source '{} even with proxy",
                source.url);
    return Err(())
}

pub(super) fn cache_source(
    source: &Source,
    integ_files: &Vec<super::cksums::IntegFile>,
    skipint: bool,
    proxy: Option<&str>
) -> Result<(), ()> 
{
    println!("Caching '{}'", source.url);
    assert!(integ_files.len() > 0, "No integ files");
    let mut good_files = vec![];
    let mut bad_files = vec![];
    for integ_file in integ_files.iter() {
        println!("Caching '{}' to '{}'",
            source.url,
            integ_file.get_path().display());
        if integ_file.valid(skipint) {
            good_files.push(integ_file);
        } else {
            bad_files.push(integ_file);
        }
    }
    let bad_count = bad_files.len();
    if bad_count > 0 {
        println!("Missing integ files for '{}': {}",
                source.url, bad_count);
    } else {
        println!("All integ files healthy for '{}'", source.url);
        return Ok(())
    }
    let mut bad_count = 0;
    while let Some(bad_file) = bad_files.pop() {
        let r = match good_files.last() {
            Some(good_file) =>
                bad_file.clone_file_from(good_file),
            None => download_source(
                source, bad_file, skipint, proxy),
        };
        match r {
            Ok(_) => good_files.push(bad_file),
            Err(_) => bad_count += 1,
        }
    }
    if bad_count > 0 {
        eprintln!("Bad files still existing after download for '{}' ({})",
                    source.url, bad_count);
        Err(())
    } else {
        Ok(())
    }
}


fn _cache_netfile_sources_for_domain_mt(
    netfile_sources: Vec<Source>, skipint:bool, proxy: Option<&str>
) -> Result<(), ()> {
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut bad = false;
    let mut threads: Vec<JoinHandle<Result<(), ()>>> = vec![];
    for netfile_source in netfile_sources {
        let integ_files = 
            super::IntegFile::vec_from_source(&netfile_source);
        let proxy_string_thread = proxy_string.clone();
        if let Err(_) = crate::threading::wait_if_too_busy(
            &mut threads, 10, "caching network files") 
            {
                bad = true;
            }
        threads.push(thread::spawn(move ||{
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            cache_source(&netfile_source, &integ_files, skipint, proxy)
        }));
    }
    if let Err(_) = crate::threading::wait_remaining(
        threads, "caching network files") 
    {
        bad = true;
    }
    if bad { Err(()) } else { Ok(()) }
}


fn _cache_netfile_sources_mt(
    netfile_sources: HashMap<u64, Vec<Source>>,
    skipint: bool,
    proxy: Option<&str>
) -> Result<(), ()> 
{
    ensure_parents()?;
    println!("Caching netfile sources with {} threads", netfile_sources.len());
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads: Vec<JoinHandle<Result<(), ()>>> =  vec![];
    for netfile_sources in netfile_sources.into_values() {
        let proxy_string_thread = proxy_string.clone();
        threads.push(thread::spawn(move || {
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            _cache_netfile_sources_for_domain_mt(
                netfile_sources, skipint, proxy)
        }));
    }
    let mut bad = false;
    for thread in threads {
        match thread.join() {
            Ok(r) => match r {
                Ok(_) => (),
                Err(_) => bad = true,
            },
            Err(e) => {
                eprintln!("Failed to join thread: {:?}", e);
                bad = true
            },
        }
    }
    if bad { Err(()) } else { Ok(()) }
}

fn _cache_git_sources_mt(
    git_sources_map: HashMap<u64, Vec<Source>>,
    holdgit: bool,
    proxy: Option<&str>,
    gmr: Option<&super::git::Gmr>
) -> Result<(), ()>
{
    let repos_map = match
        Source::to_repos_map(git_sources_map, "sources/git", gmr) {
            Some(repos_map) => repos_map,
            None => {
                eprintln!("Failed to convert to repos map");
                return Err(())
            },
        };
    super::git::Repo::sync_mt(
        repos_map, super::git::Refspecs::HeadsTags, holdgit, proxy)
}