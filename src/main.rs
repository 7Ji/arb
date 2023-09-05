use std::thread;
use std::collections::{HashMap, BTreeMap};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use git2::{Repository, FetchOptions, Progress, RemoteCallbacks, ProxyOptions};
use tempfile::tempdir;
use xxhash_rust::xxh3::xxh3_64;
use url::{Url, ParseError};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Arg {
    /// Optional PKGBUILDs.yaml file
    #[arg(default_value_t = String::from("PKGBUILDs.yaml"))]
    pkgbuilds: String,

    /// HTTP proxy to retry for git updating if attempt without proxy failed
    #[arg(short, long)]
    proxy: Option<String>,

    /// Hold versions of PKGBUILDs, do not update them
    #[arg(short='P', long, default_value_t = false)]
    holdpkg: bool,

    /// Hold versions of git sources, do not update them
    #[arg(short='G', long, default_value_t = false)]
    holdgit: bool
}

struct PKGBUILD {
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

fn open_or_init_bare_repo (path: &Path, url: &str) -> Option<Repository> {
    match Repository::open_bare(path) {
        Ok(repo) => Some(repo),
        Err(e) => {
            if e.class() == git2::ErrorClass::Os &&
               e.code() == git2::ErrorCode::NotFound {
                match Repository::init_bare(path) {
                    Ok(repo) => {
                        match &repo.remote_with_fetch(
                            "origin", url, "+refs/*:refs/*") {
                            Ok(_) => (),
                            Err(e) => {
                                eprintln!("Failed to add remote {}: {}", path.display(), e);
                                std::fs::remove_dir_all(path)
                                .expect(
                                    "Failed to remove dir after failed attempt");
                                return None
                            }
                        };
                        Some(repo)
                    },
                    Err(e) => {
                        eprintln!("Failed to create {}: {}", path.display(), e);
                        None
                    }
                }
            } else {
                eprintln!("Failed to open {}: {}", path.display(), e);
                None
            }
        },
    }
}

fn gcb_transfer_progress(progress: Progress<'_>) -> bool {
    let network_pct = (100 * progress.received_objects()) / progress.total_objects();
    let index_pct = (100 * progress.indexed_objects()) / progress.total_objects();
    let kbytes = progress.received_bytes() / 1024;
    if progress.received_objects() == progress.total_objects() {
        print!(
            "Resolving deltas {}/{}\r",
            progress.indexed_deltas(),
            progress.total_deltas()
        );
    } else {
        print!(
            "net {:3}% ({:4} kb, {:5}/{:5})  /  idx {:3}% ({:5}/{:5})\r",
            network_pct,
            kbytes,
            progress.received_objects(),
            progress.total_objects(),
            index_pct,
            progress.indexed_objects(),
            progress.total_objects()
        )
    }
    std::io::stdout().flush().unwrap();
    true
}


fn sync_repo(path: &Path, url: &str, proxy: Option<&str>) {
    println!("Syncing repo '{}' with '{}'", path.display(), url);
    let repo = 
        open_or_init_bare_repo(path, url)
        .expect("Failed to open or init repo");
    let mut remote = repo.remote_anonymous(&url).expect("Failed to create temporary remote");
    let mut cbs = RemoteCallbacks::new();
    cbs.sideband_progress(|log| {
            print!("Remote: {}", String::from_utf8_lossy(log));
            true
        });
    cbs.transfer_progress(gcb_transfer_progress);
    let mut fetch_opts = 
        FetchOptions::new();
    fetch_opts.download_tags(git2::AutotagOption::All)
        .prune(git2::FetchPrune::On)
        .update_fetchhead(true)
        .remote_callbacks(cbs);
    if let Err(e) = 
        remote.fetch(
            &["+refs/*:refs/*"], 
            Some(&mut fetch_opts), 
            None
    ) {
        if let Some(proxy) = proxy {
            eprintln!("Failed to fetch from remote: {}. Will use proxy to retry", e);
            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.url(proxy);
            fetch_opts.proxy_options(proxy_opts);
            remote.fetch(
                &["+refs/*:refs/*"], 
                Some(&mut fetch_opts), 
                None
            ).expect("Failed to fetch even with proxy");
        } else {
            eprintln!("Failed to fetch from remote: {}", e);
            panic!();
        }
    };
    for head in remote.list().expect("Failed to list remote") {
        if head.name() == "HEAD" {
            if let Some(target) = head.symref_target() {
                repo.set_head(target).expect("Failed to set head");
            }
        }
    }
}

fn read_pkgbuilds_yaml(yaml: &str) -> Vec<PKGBUILD> {
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
        let mut git = PathBuf::from("sources/git");
        git.push(format!("{:016x}", hash_url));
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
                sync_repo(&pkgbuild.git, &pkgbuild.url, proxy);
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
                sync_repo(&repo.path, &repo.url, Some(&proxy_url));
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
        match open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url) {
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

fn dump_pkgbuilds(dir: &Path, pkgbuilds: &Vec<PKGBUILD>) {
    for pkgbuild in pkgbuilds.iter() {
        let path = dir.join(&pkgbuild.name);
        let repo = 
            open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url)
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

fn main() {
    let arg = Arg::parse();
    let pkgbuilds = read_pkgbuilds_yaml(&arg.pkgbuilds);
    let update_pkg = if arg.holdpkg {
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
    let pkgbuilds_dir = tempdir().expect("Failed to create temp dir to dump PKGBUILDs");
    println!("{}", pkgbuilds_dir.path().display());
    dump_pkgbuilds(&pkgbuilds_dir.path(), &pkgbuilds);
}
