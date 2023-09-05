use std::{path::{Path, PathBuf}, io::Write};

use git2::{Repository, Progress, RemoteCallbacks, FetchOptions, ProxyOptions};

pub(crate) fn open_or_init_bare_repo (path: &Path, url: &str) -> Option<Repository> {
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


pub(crate) fn sync_repo(path: &Path, url: &str, proxy: Option<&str>) {
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
