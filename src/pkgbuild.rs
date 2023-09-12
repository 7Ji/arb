use std::{path::{PathBuf, Path}, collections::{BTreeMap, HashMap}, thread, io::Write};

use git2::Repository;
use url::Url;
use xxhash_rust::xxh3::xxh3_64;
use crate::{git, source};

#[derive(Clone)]
pub(crate) struct PKGBUILD {
    name: String,
    url: String,
    hash_url: u64,
    hash_domain: u64,
    build: PathBuf,
    git: PathBuf,
}

struct Repo {
    path: PathBuf,
    url: String,
}

fn read_pkgbuilds_yaml<P>(yaml: P) -> Vec<PKGBUILD>
where 
    P: AsRef<Path>
{
    let f = std::fs::File::open(yaml)
            .expect("Failed to open pkgbuilds YAML config");
    let config: BTreeMap<String, String> = 
        serde_yaml::from_reader(f)
            .expect("Failed to parse into config");
    config.iter().map(|(name, url)| {
        let url_p = Url::parse(url).expect("Invalid URL");
        let hash_domain = match url_p.domain() {
            Some(domain) => xxh3_64(domain.as_bytes()),
            None => 0,
        };
        let hash_url = xxh3_64(url.as_bytes());
        let mut build = PathBuf::from("build");
        build.push(name);
        let git = PathBuf::from(format!("sources/PKGBUILDs/{}", name));
        PKGBUILD {
            name: name.clone(),
            url: url.clone(),
            hash_url,
            hash_domain,
            build,
            git
        }
    }).collect()
}

fn sync_pkgbuilds(pkgbuilds: &Vec<PKGBUILD>, proxy: Option<&str>) {
    let mut map: HashMap<u64, Vec<Repo>> = HashMap::new();
    for pkgbuild in pkgbuilds.iter() {
        if ! map.contains_key(&pkgbuild.hash_domain) {
            println!("New domain found from PKGBUILD URL: {}", pkgbuild.url);
            map.insert(pkgbuild.hash_domain, vec![]);
        }
        let vec = map
            .get_mut(&pkgbuild.hash_domain)
            .expect("Failed to get vec");
        vec.push(Repo { path: pkgbuild.git.clone(), url: pkgbuild.url.clone() });
    }
    println!("Syncing PKGBUILDs with {} threads", map.len());
    const REFSPECS: &[&str] = &["+refs/heads/master:refs/heads/master"];
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads =  Vec::new();
    for repos in map.into_values() {
        let proxy_string_thread = proxy_string.clone();
        threads.push(thread::spawn(move || {
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            for repo in repos {
                git::sync_repo(&repo.path, &repo.url, proxy, REFSPECS);
            }
        }));
    }
    for thread in threads.into_iter() {
        thread.join().expect("Failed to join");
    }
}

fn get_pkgbuild_blob(repo: &Repository) -> Option<git2::Blob> {
    let branch = 
        match repo.find_branch("master", git2::BranchType::Local) {
            Ok(branch) => branch,
            Err(e) => {
                eprintln!("Failed to find master branch: {}", e);
                return None
            }
        };
    let commit = 
        match branch.get().peel_to_commit() {
            Ok(commit) => commit,
            Err(e) => {
                eprintln!("Failed to peel master branch to commit: {}", e);
                return None
            },
        };
    let tree = 
        match commit.tree() {
            Ok(tree) => tree,
            Err(e) => {
                eprintln!("Failed to get tree pointed by commit: {}", e);
                return None
            },
        };
    let entry = 
        match tree.get_name("PKGBUILD") {
            Some(entry) => entry,
            None => {
                eprintln!("Failed to find entry of PKGBUILD");
                return None
            },
        };
    let object = 
        match entry.to_object(&repo) {
            Ok(object) => object,
            Err(e) => {
                eprintln!("Failed to convert tree entry to object: {}", e);
                return None
            },
        };
    let blob = 
        match object.into_blob() {
            Ok(blob) => blob,
            Err(_) => {
                eprintln!("Failed to convert into a blob");
                return None
            },
        };
    Some(blob)
}

fn healthy_pkgbuild(pkgbuild: &PKGBUILD) -> bool {
    let repo = 
        match git::open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url) {
            Some(repo) => repo,
            None => {
                eprintln!("Failed to open or init bare repo {}", pkgbuild.git.display());
                return false
            }
        };
    let _blob = 
        match get_pkgbuild_blob(&repo) {
            Some(blob) => blob,
            None => {
                eprintln!("Failed to get PKGBUILD blob");
                return false
            },
        };
    true
}

fn healthy_pkgbuilds(pkgbuilds: &Vec<PKGBUILD>) -> bool {
    for pkgbuild in pkgbuilds.iter() {
        if ! healthy_pkgbuild(pkgbuild) {
            return false;
        }
    }
    true
}

fn dump_pkgbuilds<P> (dir: P, pkgbuilds: &Vec<PKGBUILD>)
where 
    P: AsRef<Path> 
{
    let dir = dir.as_ref();
    for pkgbuild in pkgbuilds.iter() {
        let path = dir.join(&pkgbuild.name);
        let repo = 
            git::open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url)
            .expect("Failed to open repo");
        let blob = 
            get_pkgbuild_blob(&repo)
            .expect("Failed to get PKGBUILD blob");
        let mut file = 
            std::fs::File::create(path)
            .expect("Failed to create file");
        file.write_all(blob.content()).expect("Failed to write");
    }
}

fn get_all_sources<P> (dir: P, pkgbuilds: &Vec<PKGBUILD>) 
    -> (Vec<source::Source>, Vec<source::Source>, Vec<source::Source>)
where 
    P: AsRef<Path> 
{
    let dir = dir.as_ref();
    let sources_all: Vec<Vec<source::Source>> = pkgbuilds.iter().map(|pkgbuild| {
        source::get_sources::<P>(&dir.join(&pkgbuild.name))
    }).collect();
    let mut sources_non_unique = vec![];
    for sources in sources_all.iter() {
        for source in sources.iter() {
            sources_non_unique.push(source);
        }
    }
    source::unique_sources(&sources_non_unique)
}

pub(crate) fn get_pkgbuilds<P>(config: P, hold: bool, proxy: Option<&str>) -> Vec<PKGBUILD>
where 
    P:AsRef<Path>
{
    let pkgbuilds = read_pkgbuilds_yaml(config);
    let update_pkg = if hold {
        if healthy_pkgbuilds(&pkgbuilds) {
            println!("Holdpkg set and all PKGBUILDs healthy, no need to update");
            false
        } else {
            eprintln!("Warning: holdpkg set, but unhealthy PKGBUILDs found, still need to update");
            true
        }
    } else {
        true
    };
    if update_pkg {
        sync_pkgbuilds(&pkgbuilds, proxy);
        if ! healthy_pkgbuilds(&pkgbuilds) {
            panic!("Updating broke some of our PKGBUILDs");
        }
    }
    pkgbuilds
}

pub(crate) fn prepare_sources<P>(dir: P, pkgbuilds: &Vec<PKGBUILD>, proxy: Option<&str>) 
where
    P:AsRef<Path> 
{
    dump_pkgbuilds(&dir, &pkgbuilds);
    let (netfile_sources, git_sources, local_sources) 
        = get_all_sources(&dir, &pkgbuilds);
    source::cache_sources_mt(&netfile_sources, &git_sources, proxy);
}