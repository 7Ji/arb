use std::{path::Path, io::Write};

use git2::{Repository, Progress, RemoteCallbacks, FetchOptions, ProxyOptions, Remote, Tree, Blob};

fn init_bare_repo<P: AsRef<Path>> (path: P, url: &str) -> Option<Repository> {
    let path = path.as_ref();
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
}

pub(crate) fn open_or_init_bare_repo<P: AsRef<Path>> (path: P, url: &str) -> Option<Repository> {
    let path = path.as_ref();
    match Repository::open_bare(path) {
        Ok(repo) => Some(repo),
        Err(e) => {
            if e.class() == git2::ErrorClass::Os &&
               e.code() == git2::ErrorCode::NotFound {
                init_bare_repo(path, url)
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

fn fetch_opts_init<'a>() -> FetchOptions<'a> {
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
    fetch_opts
}

fn fetch_repo(remote: &mut Remote, fetch_opts: &mut FetchOptions, proxy: Option<&str>, refspecs: &[&str]) {
    for _ in 0..2 {
        match remote.fetch(refspecs, Some(fetch_opts), None) {
            Ok(_) => return,
            Err(e) => {
                eprintln!("Failed to fetch from remote '{}': {}", remote.url().expect("Failed to get URL"), e);
            },
        }
    }
    match proxy {
        Some(proxy) => {
            eprintln!("Failed to fetch from remote '{}'. Will use proxy to retry", remote.url().expect("Failed to get URL"));
            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.url(proxy);
            fetch_opts.proxy_options(proxy_opts);
            for _ in 0..2 {
                match remote.fetch(refspecs, Some(fetch_opts), None) {
                    Ok(_) => return,
                    Err(e) => {
                        eprintln!("Failed to fetch from remote '{}': {}", remote.url().expect("Failed to get URL"), e);
                    },
                }
            }
        }
        None => {
            eprintln!("Failed to fetch from remote '{}' after 3 retries, considered failure", remote.url().expect("Failed to get URL"));
            panic!("Failed to fecth from remote and there's no proxy to retry");
        },
    }
    panic!("Failed to fetch even with proxy");
}

fn update_head(remote: &Remote, repo: &Repository) {
    for head in remote.list().expect("Failed to list remote") {
        if head.name() == "HEAD" {
            if let Some(target) = head.symref_target() {
                repo.set_head(target).expect("Failed to set head");
            }
        }
    }
}

pub(crate) fn sync_repo<P: AsRef<Path>>(path: P, url: &str, proxy: Option<&str>, refspecs: &[&str]) {
    let path = path.as_ref();
    println!("Syncing repo '{}' with '{}'", path.display(), url);
    let repo = 
        open_or_init_bare_repo(path, url)
        .expect("Failed to open or init repo");
    let mut remote = repo.remote_anonymous(&url).expect("Failed to create temporary remote");
    let mut fetch_opts = fetch_opts_init();
    fetch_repo(&mut remote, &mut fetch_opts, proxy, refspecs);
    update_head(&remote, &repo);
}

fn get_branch_tree<'a>(repo: &'a Repository, branch: &str) -> Option<Tree<'a>> {
    let branch = 
        match repo.find_branch(branch, git2::BranchType::Local) {
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
    Some(tree)
}

fn get_tree_entry_blob<'a>(repo: &'a Repository,tree: &Tree, name: &str) -> Option<Blob<'a>> {
    let entry = 
        match tree.get_name(name) {
            Some(entry) => entry,
            None => {
                eprintln!("Failed to find entry of {}", name);
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

pub(crate) fn get_branch_entry_blob<'a>(repo: &'a Repository, branch: &str, name: &str) -> Option<Blob<'a>> {
    let tree = match get_branch_tree(repo, branch) {
        Some(tree) => tree,
        None => return None,
    };
    get_tree_entry_blob(repo, &tree, name)
}

pub(crate) fn healthy_repo<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    let repo = match Repository::open_bare(&path) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("Failed to open bare repo at '{}': {}", path.display(), e);
            return false
        },
    };
    let head = match repo.head() {
        Ok(head) => head,
        Err(e) => {
            eprintln!("Failed to get head of repo '{}': {}", path.display(), e);
            return false 
        },
    };
    return match head.peel_to_commit() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Failed to get head of repo '{}': {}", path.display(), e);
            false
        },
    };
}