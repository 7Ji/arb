use std::cell::RefCell;
use std::collections::{HashMap, BTreeMap};
use std::thread;
use git2::{Repository, FetchOptions, Progress, RemoteCallbacks, ProxyOptions};
use xxhash_rust::xxh3::xxh3_64;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use url::{Url, ParseError};

struct PKGBUILD {
    name: String,
    url: String,
    hash_url: u64,
    hash_domain: u64,
    build: String,
    git: String,
}

struct Repo {
    path: String,
    url: String,
}

fn open_or_init_bare_repo (path: &str, url: &str) -> Option<Repository> {
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
                                eprintln!("Failed to add remote {}: {}", path, e);
                                std::fs::remove_dir_all(path)
                                .expect(
                                    "Failed to remove dir after failed attempt");
                                return None
                            }
                        };
                        Some(repo)
                    },
                    Err(e) => {
                        eprintln!("Failed to create {}: {}", path, e);
                        None
                    }
                }
            } else {
                eprintln!("Failed to open {}: {}", path, e);
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


fn sync_repo(path: &str, url: &str, proxy: Option<&str>) {
    println!("Syncing repo '{}' with '{}'", path, url);
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
    if let Err(_) = 
        remote.fetch(
            &["+refs/*:refs/*"], 
            Some(&mut fetch_opts), 
            None
    ) {
        if let Some(proxy) = proxy {
            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.url(proxy);
            fetch_opts.proxy_options(proxy_opts);
            remote.fetch(
                &["+refs/*:refs/*"], 
                Some(&mut fetch_opts), 
                None
            ).expect("Failed to fetch even with proxy");
        }
    };
    for head in remote.list().expect("Failed to list remote") {
        if head.name() == "HEAD" {
            if let Some(target) = head.symref_target() {
                repo.set_head(target).expect("Failed to set head");
            }
        }
    }
    // For a PKGBUILD.git, there must be a PKGBUILD available on HEAD
    // let blob_obj = repo.head()
    //     .expect("Failed to lookup head")
    //     .peel_to_commit()
    //     .expect("Failed to peel to commit")
    //     .tree()
    //     .expect("Failed to get tree")
    //     .get_name("PKGBUILD")
    //     .expect("Failed to lookup PKGBUILD")
    //     .to_object(&repo)
    //     .expect("Failed to convert to object");
    // let blob = blob_obj
    //     .as_blob()
    //     .expect("Failed to convert to blob");
    // std::os::unix::
    // print!("{}\n", String::from_utf8_lossy((blob.content())));

}

// struct State {
//     progress: Option<Progress<'static>>,
//     total: usize,
//     current: usize,
//     path: Option<PathBuf>,
//     newline: bool,
// }

// impl Repo for PKGBUILD {
//     fn sync(&self) {

//         // let state = RefCell::new(State {
//         //     progress: None,
//         //     total: 0,
//         //     current: 0,
//         //     path: None,
//         //     newline: false,
//         // });
//         // let repo = open_or_init_bare_repo(&self.git, &self.url)
//         //     .expect("Failed to open or init repo");
        
//         // let mut remote = repo.remote_anonymous(&self.url).expect("Failed to find origin remote");
//         // let mut cbs = RemoteCallbacks::new();
//         // cbs.sideband_progress(|log| {
//         //         print!("Remote: {}", String::from_utf8_lossy(log));
//         //         true
//         //     });
//         // cbs.transfer_progress(|progress| {
//         //     let mut state = state.borrow_mut();
//         //     state.progress = Some(progress.to_owned());
//         //     print(&mut *state);
//         //     true
//         // });
//         // let mut opts = 
//         //     FetchOptions::new();
//         // opts.download_tags(git2::AutotagOption::All)
//         //     .prune(git2::FetchPrune::On)
//         //     .update_fetchhead(true)
//         //     .remote_callbacks(cbs);
//         // remote.fetch(&["+refs/*:refs/*"], Some(&mut opts), None).expect("Failed to update");
//         // for head in remote.list().expect("Failed to list remote") {
//         //     if head.name() == "HEAD" {
//         //         if let Some(target) = head.symref_target() {
//         //             repo.set_head(target).expect("Failed to set head");
//         //         }
//         //     }
//         // }
//         // // For a PKGBUILD.git, there must be a PKGBUILD available on HEAD
//         // let blob_obj = repo.head()
//         //     .expect("Failed to lookup head")
//         //     .peel_to_commit()
//         //     .expect("Failed to peel to commit")
//         //     .tree()
//         //     .expect("Failed to get tree")
//         //     .get_name("PKGBUILD")
//         //     .expect("Failed to lookup PKGBUILD")
//         //     .to_object(&repo)
//         //     .expect("Failed to convert to object");
//         // let blob = blob_obj
//         //     .as_blob()
//         //     .expect("Failed to convert to blob");
//         // // std::os::unix::
//         // // print!("{}\n", String::from_utf8_lossy((blob.content())));
//     }
// }

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
        let build = format!("build/{}", name);
        let git = format!("sources/git/{:016x}", hash_url);
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
    let mut threads =  Vec::new();
    for (_, pkgbuilds) in map.iter() {
        let mut repos = vec![];
        for pkgbuild in pkgbuilds.iter() {
            repos.push(Repo {
                path: pkgbuild.git.clone(),
                url: pkgbuild.url.clone(),
            });
        }
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

fn main() {
    let pkgbuilds = 
        match std::env::args().nth(1) {
            Some(path) => read_pkgbuilds_yaml(&path),
            None => {
                println!("Warning: pkgbuild.yaml not set on CLI, use default pkgbuild.yaml");
                read_pkgbuilds_yaml("pkgbuild.yaml")
            },
        };
    sync_pkgbuilds(&pkgbuilds, Some("http://xray.lan:1081"));
}
