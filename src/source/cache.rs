use crate::threading;
use std::{
        collections::HashMap,
        thread::{
            self,
            JoinHandle,
        }
    };
use super::{
    netfile,
    cksums::IntegFile,
    git::ToReposMap,
    Source,
    MapByDomain
};

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
    actual_identity: &crate::identity::Identity,
    holdgit: bool,
    skipint: bool,
    proxy: Option<&str>,
    gmr: Option<&super::git::Gmr>
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
                let actual_identity_thread = actual_identity.clone();
                let netfile_thread = thread::spawn(
                move ||{
                    let proxy = match has_proxy {
                        true => Some(proxy_string_thread.as_str()),
                        false => None,
                    };
                    netfile::cache_source(&netfile_source, &integ_files,
                         &actual_identity_thread, skipint, proxy)
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
                    git_repo.sync(proxy, super::git::Refspecs::HeadsTags)
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
