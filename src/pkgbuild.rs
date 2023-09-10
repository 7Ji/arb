use std::{path::{PathBuf, Path}, collections::{BTreeMap, HashMap}, thread, io::Write};

use git2::Repository;
use url::Url;
use xxhash_rust::xxh3::xxh3_64;
use crate::{git, source};

pub(crate) struct PKGBUILD {
    name: String,
    url: String,
    _hash_url: u64,
    hash_domain: u64,
    _build: PathBuf,
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
        let _hash_url = xxh3_64(url.as_bytes());
        let mut _build = PathBuf::from("build");
        _build.push(name);
        let mut git = PathBuf::from("sources/git");
        git.push(format!("{:016x}", _hash_url));
        PKGBUILD {
            name: name.clone(),
            url: url.clone(),
            _hash_url,
            hash_domain,
            _build,
            git
        }
    }).collect()
}

fn sync_pkgbuilds(pkgbuilds: &Vec<PKGBUILD>, proxy: Option<&str>) {
    let mut map: HashMap<u64, Vec<&PKGBUILD>> = HashMap::new();
    for pkgbuild in pkgbuilds.iter() {
        if ! map.contains_key(&pkgbuild.hash_domain) {
            map.insert(pkgbuild.hash_domain, vec![]);
        }
        let vec = map
            .get_mut(&pkgbuild.hash_domain)
            .expect("Failed to get vec");
        vec.push(pkgbuild);
    }
    match pkgbuilds.len() {
        0 => {
            panic!("No PKGBUILDs defined");
        },
        1 => {
            for pkgbuild in pkgbuilds {
                git::sync_repo(&pkgbuild.git, &pkgbuild.url, proxy);
            }
            return
        },
        _ => ()
    }
    println!("Syncing PKGBUILDs with {} threads, one thread per domain...", 
            map.len());
    let mut threads =  Vec::new();
    for (domain, pkgbuilds) in map.iter() {
        print!("PKGBUILDs from domain {:016x}:", domain);
        let mut repos = vec![];
        for pkgbuild in pkgbuilds.iter() {
            print!(" '{}'", pkgbuild.name);
            repos.push(Repo {
                path: pkgbuild.git.clone(),
                url: pkgbuild.url.clone(),
            });
        }
        println!();
        let proxy_url = match proxy {
            Some(proxy_url) => proxy_url.to_string(),
            None => String::new(),
        };
        threads.push(thread::spawn(move || {
            for repo in repos {
                git::sync_repo(&repo.path, &repo.url, Some(&proxy_url));
            }
        }));
    }
    for handle in threads {
        handle.join().expect("Failed to join");
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
where 
    P: AsRef<Path> 
{
    let dir = dir.as_ref();
    let mut sources_all = vec![];
    for pkgbuild in pkgbuilds.iter() {
        let mut sources_this = source::get_sources::<P>(&dir.join(&pkgbuild.name));
        sources_all.append(&mut sources_this);
    }
    sources_all = source::dedup_sources(&sources_all);
    source::cache_sources(&sources_all);
}

pub(crate) fn get_pkgbuilds<P>(config: P, hold: bool) -> Vec<PKGBUILD>
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
        sync_pkgbuilds(&pkgbuilds, Some("http://xray.lan:1081"));
        if ! healthy_pkgbuilds(&pkgbuilds) {
            panic!("Updating broke some of our PKGBUILDs");
        }
    }
    pkgbuilds
}

pub(crate) fn prepare_sources<P>(dir: P, pkgbuilds: &Vec<PKGBUILD>) 
where
    P:AsRef<Path> 
{
    dump_pkgbuilds(&dir, &pkgbuilds);
    get_all_sources(&dir, &pkgbuilds);
}