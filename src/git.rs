use crate::threading;
use git2::{
        Blob,
        Branch,
        build::CheckoutBuilder,
        Commit,
        FetchOptions,
        Oid,
        Remote,
        RemoteCallbacks,
        Repository,
        Progress,
        ProxyOptions,
        Tree,
    };
use std::{
        collections::HashMap, 
        io::Write, 
        path::{
            Path, 
            PathBuf
        },
        thread::{
            self,
            JoinHandle
        }
    };
use xxhash_rust::xxh3::xxh3_64;

const REFSPECS_HEADS_TAGS: &[&str] = &[
    "+refs/heads/*:refs/heads/*", 
    "+refs/tags/*:refs/tags/*"
];

const REFSPECS_MASTER_ONLY: &[&str] = &["+refs/heads/master:refs/heads/master"];

#[derive(Clone)]
pub(crate) enum Refspecs {
    HeadsTags,
    MasterOnly
}

impl Refspecs {
    fn get(&self) -> &[&str] {
        match self {
            Refspecs::HeadsTags => REFSPECS_HEADS_TAGS,
            Refspecs::MasterOnly => REFSPECS_MASTER_ONLY,
        }
    }
}

pub(crate) struct Repo {
    path: PathBuf,
    url: String,
    repo: Repository,
}

pub(crate) trait ToReposMap {
    fn url(&self) -> &str;
    fn path(&self) -> Option<&Path>;
    fn to_repos_map(map: HashMap<u64, Vec<Self>>, parent: &str)
        -> HashMap<u64, Vec<Repo>> 
    where Self: Sized{
        let mut repos_map = HashMap::new();
        let parent = PathBuf::from(parent);
        for (domain, sources) in map {
            let repos: Vec<Repo> = sources.iter().map(|source| {
                let url = source.url();
                let repo = match source.path() {
                    Some(path) => Repo::open_bare(path, url),
                    None => {
                        let path = parent.join(
                            format!(
                                    "{:016x}",
                                    xxh3_64(source.url().as_bytes())));
                        Repo::open_bare(&path, url)
                    },
                };
                match repo {
                    Some(repo) => repo,
                    None => {
                        eprintln!(
                            "Failed to open bare repo for git source '{}'",
                            url);
                        panic!("Failed to open bare repo");
                    },
                }
            }).collect();
            match repos_map.insert(domain, repos) {
                Some(_) => panic!("Duplicated key for repos map"),
                None => (),
            }
        }
        repos_map
    }
}

fn gcb_transfer_progress(progress: Progress<'_>) -> bool {
    let network_pct = 
        (100 * progress.received_objects()) / progress.total_objects();
    let index_pct =
        (100 * progress.indexed_objects()) / progress.total_objects();
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

fn fetch_remote(
    remote: &mut Remote,
    fetch_opts: &mut FetchOptions, 
    proxy: Option<&str>,
    refspecs: &[&str]
) {
    for _ in 0..2 {
        match remote.fetch(refspecs, Some(fetch_opts), None) {
            Ok(_) => return,
            Err(e) => {
                eprintln!("Failed to fetch from remote '{}': {}",
                        remote.url().expect("Failed to get URL"), e);
            },
        }
    }
    match proxy {
        Some(proxy) => {
            eprintln!(
                "Failed to fetch from remote '{}'. Will use proxy to retry",
                remote.url().expect("Failed to get URL"));
            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.url(proxy);
            fetch_opts.proxy_options(proxy_opts);
            for _ in 0..2 {
                match remote.fetch(
                    refspecs, Some(fetch_opts), None) {
                    Ok(_) => return,
                    Err(e) => {
                        eprintln!("Failed to fetch from remote '{}': {}",
                            remote.url().expect("Failed to get URL"), e);
                    },
                }
            }
        }
        None => {
            eprintln!("Failed to fetch from remote '{}' after 3 retries",
                        remote.url().expect("Failed to get URL"));
            panic!("Failed to fecth from remote and there's no proxy to retry");
        },
    }
    panic!("Failed to fetch even with proxy");
}

impl Repo {
    fn add_remote(&self) -> bool {
        match &self.repo.remote_with_fetch(
            "origin", &self.url, "+refs/*:refs/*") {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Failed to add remote {}: {}",
                            self.path.display(), e);
                std::fs::remove_dir_all(&self.path)
                .expect(
                    "Failed to remove dir after failed attempt");
                false
            }
        }
    }

    fn init_bare<P: AsRef<Path>>(path: P, url: &str) -> Option<Self> {
        match Repository::init_bare(&path) {
            Ok(repo) => {
                let repo = Self {
                    path: path.as_ref().to_owned(),
                    url: url.to_owned(),
                    repo,
                };
                if repo.add_remote() {
                    Some(repo)
                } else {
                    None
                }
            },
            Err(e) => {
                eprintln!("Failed to create {}: {}", 
                            &path.as_ref().display(), e);
                None
            }
        }
    }

    pub(crate) fn open_bare<P: AsRef<Path>>(path: P, url: &str) 
        -> Option<Self> 
    {
        match Repository::open_bare(&path) {
            Ok(repo) => Some(Self {
                path: path.as_ref().to_owned(),
                url: url.to_owned(),
                repo,
            }),
            Err(e) => {
                if e.class() == git2::ErrorClass::Os &&
                e.code() == git2::ErrorCode::NotFound {
                    Self::init_bare(path, url)
                } else {
                    eprintln!("Failed to open {}: {}", 
                            path.as_ref().display(), e);
                    None
                }
            },
        }
    }

    fn update_head(&self, remote: &Remote) {
        let heads = 
                remote.list().expect("Failed to list remote");
        for head in heads {
            if head.name() == "HEAD" {
                if let Some(target) = head.symref_target() {
                    self.repo.set_head(target)
                             .expect("Failed to set head");
                }
            }
        }
    }

    fn sync(&self, proxy: Option<&str>, refspecs: &[&str]) {
        println!("Syncing repo '{}' with '{}'", 
                    &self.path.display(), &self.url);
        let mut remote = 
            self.repo.remote_anonymous(&self.url)
            .expect("Failed to create temporary remote");
        let mut fetch_opts = fetch_opts_init();
        fetch_remote(&mut remote, &mut fetch_opts, proxy, refspecs);
        self.update_head(&remote);
    }

    fn get_branch<'a>(&'a self, branch: &str) -> Option<Branch<'a>> {
        match self.repo.find_branch(branch, git2::BranchType::Local) {
            Ok(branch) => Some(branch),
            Err(e) => {
                eprintln!("Failed to find master branch: {}", e);
                None
            }
        }
    }

    fn get_branch_commit<'a>(&'a self, branch: &str) -> Option<Commit<'a>> {
        let branch = match self.get_branch(branch) {
            Some(branch) => branch,
            None => return None,
        };
        match branch.get().peel_to_commit() {
            Ok(commit) => Some(commit),
            Err(e) => {
                eprintln!("Failed to peel master branch to commit: {}", e);
                return None
            },
        }
    }
    
    pub(crate) fn get_branch_commit_id(&self, branch: &str) -> Option<Oid> {
        match self.get_branch_commit(branch) {
            Some(commit) => Some(commit.id()),
            None => None,
        }
    }
    
    fn get_branch_tree<'a>(&'a self, branch: &str) -> Option<Tree<'a>> {
        let commit = match self.get_branch_commit(branch) {
            Some(commit) => commit,
            None => {
                eprintln!("Failed to get commit pointed by branch {}", branch);
                return None
            },
        };
        match commit.tree() {
            Ok(tree) => Some(tree),
            Err(e) => {
                eprintln!("Failed to get tree pointed by commit: {}", e);
                return None
            },
        }
    }
    
    fn get_tree_entry_blob<'a>(&'a self, tree: &Tree, name: &str)
        -> Option<Blob<'a>> 
    {
        let entry = 
            match tree.get_name(name) {
                Some(entry) => entry,
                None => {
                    eprintln!("Failed to find entry of {}", name);
                    return None
                },
            };
        let object = 
            match entry.to_object(&self.repo) {
                Ok(object) => object,
                Err(e) => {
                    eprintln!("Failed to convert tree entry to object: {}", e);
                    return None
                },
            };
        match object.into_blob() {
            Ok(blob) => Some(blob),
            Err(_) => {
                eprintln!("Failed to convert into a blob");
                return None
            },
        }
    }
    
    pub(crate) fn get_branch_entry_blob<'a>(&'a self, branch: &str, name: &str)
        -> Option<Blob<'a>> 
    {
        let tree = match self.get_branch_tree(branch) {
            Some(tree) => tree,
            None => return None,
        };
        self.get_tree_entry_blob(&tree, name)
    }

    pub(crate) fn get_pkgbuild_blob(&self) -> Option<git2::Blob> {        
        self.get_branch_entry_blob("master", "PKGBUILD")
    }
    
    pub(crate) fn healthy(&self) -> bool {
        let head = match self.repo.head() {
            Ok(head) => head,
            Err(e) => {
                eprintln!("Failed to get head of repo '{}': {}",
                        self.path.display(), e);
                return false 
            },
        };
        return match head.peel_to_commit() {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Failed to get head of repo '{}': {}",
                        self.path.display(), e);
                false
            },
        };
    }
    
    pub(crate) fn checkout_branch<P>(&self, target: P, branch: &str) 
    where
        P: AsRef<Path>
    {
        let tree = self.get_branch_tree(branch)
                                 .expect("Failed to get commit");
        self.repo.cleanup_state().expect("Failed to cleanup state");
        self.repo.set_workdir(
                    target.as_ref(),
                    false)
                 .expect("Failed to set work dir");
        self.repo.checkout_tree(
                    tree.as_object(),
                    Some(CheckoutBuilder::new().force()))
                 .expect("Failed to checkout tree");
    }

    fn sync_for_domain(
        repos: Vec<Self>,
        refspecs: Refspecs,
        max_threads: usize,
        hold: bool,
        proxy: Option<&str>
    ) {
        let (proxy_string, has_proxy) = match proxy {
            Some(proxy) => (proxy.to_owned(), true),
            None => (String::new(), false),
        };
        let mut threads: Vec<JoinHandle<()>> = vec![];
        for repo in repos {
            if hold {
                if repo.healthy() {
                    continue;
                } else {
                    println!(
                        "Holdgit set but repo '{}' not healthy, need update",
                        repo.path.display());
                }
            }
            let proxy_string_thread = proxy_string.clone();
            threading::wait_if_too_busy(&mut threads, max_threads);
            let refspecs = refspecs.clone();
            threads.push(thread::spawn(move ||{
                let proxy = match has_proxy {
                    true => Some(proxy_string_thread.as_str()),
                    false => None,
                };
                repo.sync(proxy, refspecs.get())
            }));
        }
        for thread in threads.into_iter() {
            thread.join().expect("Failed to join finished thread");
        }
    }

    pub(crate) fn sync_mt(
        repos_map: HashMap<u64, Vec<Self>>,
        refspecs: Refspecs,
        hold: bool,
        proxy: Option<&str>
    ) {
        println!("Syncing repos with {} groups", repos_map.len());
        let (proxy_string, has_proxy) = match proxy {
            Some(proxy) => (proxy.to_owned(), true),
            None => (String::new(), false),
        };
        let mut threads: Vec<std::thread::JoinHandle<()>> =  Vec::new();
        for (domain, repos) in repos_map {
            let max_threads = match domain {
                0xb463cbdec08d6265 => 1, // aur.archlinux.org
                _ => 4
            };
            println!("Max {} threads syncing for domain 0x{:x}",
                     max_threads, domain);
            let proxy_string_thread = proxy_string.clone();
            let refspecs = refspecs.clone();
            threads.push(thread::spawn(move || {
                let proxy = match has_proxy {
                    true => Some(proxy_string_thread.as_str()),
                    false => None,
                };
                Self::sync_for_domain(
                    repos, refspecs, max_threads, hold, proxy);
            }));
        }
        for thread in threads {
            thread.join().expect("Failed to join git cacher threads");
        }
    }
}