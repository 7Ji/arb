use std::{path::{PathBuf, Path}, collections::{BTreeMap, HashMap}, thread::{self, sleep, JoinHandle}, io::Write, process::Command, fs::{DirBuilder, remove_dir_all, create_dir_all}, time::Duration};

use git2::{Repository, Oid};
use url::Url;
use xxhash_rust::xxh3::xxh3_64;
use crate::{git, source, threading};

#[derive(Clone)]
enum Pkgver {
    Plain,
    Func { pkgver: String },
}

#[derive(Clone)]
pub(crate) struct PKGBUILD {
    name: String,
    url: String,
    hash_url: u64,
    hash_domain: u64,
    build: PathBuf,
    git: PathBuf,
    pkg: PathBuf,
    commit: git2::Oid,
    pkgver: Pkgver,
    extract: bool,
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
            git,
            pkg: PathBuf::from("pkgs"),
            commit: Oid::zero(),
            pkgver: Pkgver::Plain,
            extract: false
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
    git::get_branch_entry_blob(repo, "master", "PKGBUILD")
}

fn healthy_pkgbuild(pkgbuild: &mut PKGBUILD, set_commit: bool) -> bool {
    let repo = 
        match git::open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url) {
            Some(repo) => repo,
            None => {
                eprintln!("Failed to open or init bare repo {}", pkgbuild.git.display());
                return false
            }
        };
    if set_commit {
        match git::get_branch_commit_id(&repo, "master") {
            Some(id) => pkgbuild.commit = id,
            None => {
                eprintln!("Failed to set commit id for pkgbuild {}", pkgbuild.name);
                return false
            },
        }
    }
    println!("PKGBUILD '{}' at commit '{}'", pkgbuild.name, pkgbuild.commit);
    match get_pkgbuild_blob(&repo) {
        Some(_) => return true,
        None => {
            eprintln!("Failed to get PKGBUILD blob");
            return false
        },
    };
}

fn healthy_pkgbuilds(pkgbuilds: &mut Vec<PKGBUILD>, set_commit: bool) -> bool {
    for pkgbuild in pkgbuilds.iter_mut() {
        if ! healthy_pkgbuild(pkgbuild, set_commit) {
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
    let mut pkgbuilds = read_pkgbuilds_yaml(config);
    let update_pkg = if hold {
        if healthy_pkgbuilds(&mut pkgbuilds, true) {
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
        if ! healthy_pkgbuilds(&mut pkgbuilds, true) {
            panic!("Updating broke some of our PKGBUILDs");
        }
    }
    pkgbuilds
}

fn extract_source<P: AsRef<Path>>(dir: P, repo: P) {
    create_dir_all(&dir).expect("Failed to create dir");
    git::checkout_branch_from_repo(&dir, &repo, "master");
    const SCRIPT: &str = include_str!("scripts/extract_sources.bash");
    Command::new("/bin/bash")
        .arg("-ec")
        .arg(SCRIPT)
        .arg("Source reader")
        .arg(dir.as_ref().canonicalize().expect("Failed to canonicalize dir"))
        .spawn()
        .expect("Failed to run script")
        .wait()
        .expect("Failed to wait for spawned script");
}

// fn fill_pkgver<P: AsRef<Path>>(pkgbuild: &mut PKGBUILD, pkgbuild_file: P) {
//     let output = Command::new("/bin/bash")
//         .arg("-c")
//         .arg(". \"$1\"; type -t pkgver")
//         .arg("Type Identifier")
//         .arg(pkgbuild_file.as_ref())
//         .output()
//         .expect("Failed to run script");
//     let mut pkgname = format!("{}-{}", pkgbuild.name, pkgbuild.commit);
//     match output.stdout.as_slice() {
//         b"function\n" => {
//             println!("{}'s pkgver is a function, a full deploy is needed to run it", pkgbuild.name);
//             let pkgver = String::new();
//             pkgname.push('-');
//             pkgname.push_str(&pkgver);
//             pkgbuild.pkgver = Pkgver::Func { pkgver };
//         },
//         _ => {
//             pkgbuild.pkgver = Pkgver::Plain
//         }
//     }
//     pkgbuild.pkg.push(pkgname);
// }


fn fill_all_pkgvers<P: AsRef<Path>>(dir: P, pkgbuilds: &mut Vec<PKGBUILD>) {
    let _ = remove_dir_all("build");
    let dir = dir.as_ref();
    for pkgbuild in pkgbuilds.iter_mut() {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg(". \"$1\"; type -t pkgver")
            .arg("Type Identifier")
            .arg(dir.join(&pkgbuild.name))
            .output()
            .expect("Failed to run script");
        pkgbuild.extract = match output.stdout.as_slice() {
            b"function\n" => true,
            _ => false,
        }
    }
    let mut dir_builder = DirBuilder::new();
    dir_builder.recursive(true);
    let mut threads: Vec<JoinHandle<()>> = vec![];
    for pkgbuild in pkgbuilds.iter().filter(|pkgbuild| pkgbuild.extract) {
        let dir = pkgbuild.build.clone();
        let repo = pkgbuild.git.clone();
        threading::wait_if_too_busy(&mut threads, 20);
        threads.push(thread::spawn(move|| extract_source(dir, repo)));
    }
    for thread in threads {
        thread.join().expect("Failed to join finished thread");
    }
}

// fn need_build(pkgbuild: &PKGBUILD) -> bool {

//     // pkg = PathBuf::from(format!("pkgs/{}/{}", pkgbuild.name, pkgbuild.commit));
//     // pkgbuild.pkg
//     // if pkgbuild.pkg.exists() {
//     //     let entries = pkgbuild.pkg.read_dir().iter().count();
//     //     if entries > 2 {
//     //         return false
//     //     }
//     // }
//     true
// }

pub(crate) fn prepare_sources<P: AsRef<Path>>(dir: P, pkgbuilds: &mut Vec<PKGBUILD>, holdgit: bool, skipint: bool, proxy: Option<&str>) {
    dump_pkgbuilds(&dir, &pkgbuilds);
    let (netfile_sources, git_sources, local_sources) 
        = get_all_sources(&dir, &pkgbuilds);
    source::cache_sources_mt(&netfile_sources, &git_sources, holdgit, skipint, proxy);
    fill_all_pkgvers(dir, pkgbuilds);
}