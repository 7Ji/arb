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
use url::Url;
use std::{
        collections::HashMap,
        io::Write,
        path::{
            Path,
            PathBuf
        },
        str::FromStr,
        thread,
    };

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

pub(crate) struct Gmr {
    prefix: String,
}

impl Gmr {
    pub(crate) fn init(gmr: &str) -> Self {
        Self {
            prefix: gmr.to_owned()
        }
    }

    fn get_mirror_url(&self, orig: &str) -> Option<String> {
        let orig_url = Url::from_str(orig).ok()?;
        let mut mirror_url = self.prefix.clone();
        mirror_url.push('/');
        mirror_url.push_str(orig_url.host_str()?);
        mirror_url.push_str(orig_url.path());
        Some(mirror_url)
    }
}

fn optional_gmr(gmr: Option<&Gmr>, orig: &str) -> Option<String> {
    match gmr {
        Some(gmr) => gmr.get_mirror_url(orig),
        None => None,
    }
}

pub(crate) struct Repo {
    path: PathBuf,
    url: String,
    mirror: Option<String>,
    repo: Repository,
}

pub(crate) trait ToReposMap {
    fn url(&self) -> &str;
    fn hash_url(&self) -> u64;
    fn path(&self) -> Option<&Path>;
    fn to_repo(&self, parent: &str, gmr: Option<&Gmr>) -> Option<Repo> {
        let url = self.url();
        let repo = match self.path() {
            Some(path) => Repo::open_bare(path, url, gmr),
            None => {
                let mut path = PathBuf::from(parent);
                path.push(format!("{:016x}", self.hash_url()));
                Repo::open_bare(path, url, gmr)
            },
        };
        match repo {
            Some(repo) => Some(repo),
            None => {
                eprintln!(
                    "Failed to open bare repo for git source '{}'",
                    url);
                None
            },
        }

    }
    fn to_repos_map(
        map: HashMap<u64, Vec<Self>>, parent: &str, gmr: Option<&Gmr>
    ) -> Option<HashMap<u64, Vec<Repo>>>
    where Self: Sized{
        let mut repos_map = HashMap::new();
        for (domain, sources) in map {
            let mut repos = vec![];
            for source in sources {
                match source.to_repo(parent, gmr) {
                    Some(repo) => repos.push(repo),
                    None => {
                        return None
                    },
                }
            }
            match repos_map.insert(domain, repos) {
                Some(_) => {
                    eprintln!("Duplicated key for repos map");
                    return None
                },
                None => (),
            }
        }
        Some(repos_map)
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

fn remote_safe_url<'a>(remote: &'a Remote) -> &'a str {
    match remote.url() {
        Some(url) => url,
        None => "unknown",
    }
}

fn fetch_remote(
    remote: &mut Remote,
    fetch_opts: &mut FetchOptions,
    proxy: Option<&str>,
    refspecs: &[&str]
) -> Result<(), ()> 
{
    for _ in 0..2 {
        match remote.fetch(
            refspecs, Some(fetch_opts), None
        ) {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Failed to fetch from remote '{}': {}", 
                    remote_safe_url(&remote), e);
            },
        }
    }
    match proxy {
        Some(proxy) => {
            eprintln!(
                "Failed to fetch from remote '{}'. Will use proxy to retry",
                remote_safe_url(&remote));
            let mut proxy_opts = ProxyOptions::new();
            proxy_opts.url(proxy);
            fetch_opts.proxy_options(proxy_opts);
            for _ in 0..2 {
                match remote.fetch(
                    refspecs, Some(fetch_opts), None) {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        eprintln!("Failed to fetch from remote '{}': {}",
                        remote_safe_url(&remote), e);
                    },
                }
            };
            eprintln!("Failed to fetch from remote '{}' even with proxy", 
                remote_safe_url(&remote));
        }
        None => {
            eprintln!("Failed to fetch from remote '{}' after 3 retries",
                remote_safe_url(&remote));
        },
    }
    return Err(());
}

impl Repo {
    fn add_remote(&self) -> Result<(), ()> {
        match &self.repo.remote_with_fetch(
            "origin", &self.url, "+refs/*:refs/*") {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Failed to add remote {}: {}",
                            self.path.display(), e);
                std::fs::remove_dir_all(&self.path)
                .expect(
                    "Failed to remove dir after failed attempt");
                Err(())
            }
        }
    }

    fn init_bare<P: AsRef<Path>>(path: P, url: &str, gmr: Option<&Gmr>)
        -> Option<Self> 
    {
        match Repository::init_bare(&path) {
            Ok(repo) => {
                let repo = Self {
                    path: path.as_ref().to_owned(),
                    url: url.to_owned(),
                    mirror: optional_gmr(gmr, url),
                    repo,
                };
                match repo.add_remote() {
                    Ok(_) => Some(repo),
                    Err(_) => None,
                }
            },
            Err(e) => {
                eprintln!("Failed to create {}: {}",
                            &path.as_ref().display(), e);
                None
            }
        }
    }

    pub(crate) fn open_bare<P: AsRef<Path>>(
        path: P, url: &str, gmr: Option<&Gmr>
    ) -> Option<Self>
    {
        match Repository::open_bare(&path) {
            Ok(repo) => Some(Self {
                path: path.as_ref().to_owned(),
                url: url.to_owned(),
                mirror: optional_gmr(gmr, url),
                repo,
            }),
            Err(e) => {
                if e.class() == git2::ErrorClass::Os &&
                e.code() == git2::ErrorCode::NotFound {
                    Self::init_bare(path, url, gmr)
                } else {
                    eprintln!("Failed to open {}: {}",
                            path.as_ref().display(), e);
                    None
                }
            },
        }
    }

    fn _with_gmr(&mut self, gmr: &Gmr) {
        self.mirror = gmr.get_mirror_url(&self.url)
    }

    fn update_head_raw(repo: &Repository, remote: &mut Remote) 
        -> Result<(), ()> 
    {
        let url = remote_safe_url(remote);
        let heads = match remote.list() {
            Ok(heads) => heads,
            Err(e) => {
                eprintln!("Failed to list remote '{}' for repo '{}': {}", 
                    url, repo.path().display(), e);
                return Err(())
            },
        };
        for head in heads {
            if head.name() == "HEAD" {
                if let Some(target) = head.symref_target() {
                    match repo.set_head(target) {
                        Ok(_) => return Ok(()),
                        Err(e) => {
                            eprintln!("Failed to set head for '{}': {}", 
                                        url, e);
                        },
                    }
                }
            }
        }
        Ok(())
    }

    fn _update_head(&self, remote: &mut Remote) -> Result<(), ()> {
        Self::update_head_raw(&self.repo, remote)
    }

    fn sync_raw(
        repo: &Repository, url: &str, proxy: Option<&str>, refspecs: &[&str]
    ) -> Result<(), ()> 
    {
        let mut remote =
            repo.remote_anonymous(url)
            .expect("Failed to create temporary remote");
        let mut fetch_opts = fetch_opts_init();
        fetch_remote(&mut remote, &mut fetch_opts, proxy, refspecs)?;
        Self::update_head_raw(repo, &mut remote)?;
        Ok(())
    }

    pub(crate) fn sync(&self, proxy: Option<&str>, refspecs: Refspecs)
        -> Result<(), ()>
    {
        let refspecs = refspecs.get();
        if let Some(mirror) = &self.mirror {
            println!("Syncing repo '{}' with gmr '{}' before actual remote",
                        &self.path.display(), &mirror);
            if let Ok(_) = Self::sync_raw(
                &self.repo, &mirror, None, refspecs
            ) {
                return Ok(())
            }
        }
        println!("Syncing repo '{}' with '{}' ", 
            &self.path.display(), &self.url);
        Self::sync_raw(&self.repo, &self.url, proxy, refspecs)
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

    fn get_domain(&self) -> String {
        if let Ok(url) = Url::parse(&self.url) {
            if let Some(domain) = url.domain() {
                return domain.to_string()
            }
        }
        format!("of url {}", &self.url)
    }

    fn sync_for_domain(
        repos: Vec<Self>,
        refspecs: Refspecs,
        max_threads: usize,
        hold: bool,
        proxy: Option<&str>
    ) -> Result<(), ()>
    {
        let (proxy_string, has_proxy) = match proxy {
            Some(proxy) => (proxy.to_owned(), true),
            None => (String::new(), false),
        };
        let mut threads = vec![];
        let job = format!("syncing git repos from domain '{}'", 
            repos.last().expect("Failed to get repo").get_domain());
        let mut bad = false;
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
            if let Err(_) = 
                threading::wait_if_too_busy(&mut threads, max_threads, &job) {
                bad = true;
            }
            let refspecs = refspecs.clone();
            threads.push(thread::spawn(move ||{
                let proxy = match has_proxy {
                    true => Some(proxy_string_thread.as_str()),
                    false => None,
                };
                repo.sync(proxy, refspecs)
            }));
        }
        if let Err(_) = threading::wait_remaining(threads, &job) {
            bad = true;
        }
        if bad {
            Err(())
        } else {
            Ok(())
        }
    }

    pub(crate) fn sync_mt(
        repos_map: HashMap<u64, Vec<Self>>,
        refspecs: Refspecs,
        hold: bool,
        proxy: Option<&str>
    ) -> Result<(), ()>
    {
        println!("Syncing repos with {} groups", repos_map.len());
        let (proxy_string, has_proxy) = match proxy {
            Some(proxy) => (proxy.to_owned(), true),
            None => (String::new(), false),
        };
        let mut threads = vec![];
        for (domain, repos) in repos_map {
            let max_threads = match domain {
                0xb463cbdec08d6265 => 1, // aur.archlinux.org,
                _ => 10,
            };
            println!("Max {} threads from domain {}", max_threads,
                        repos.last().expect("Failed to get repo")
                        .get_domain());
            let proxy_string_thread = proxy_string.clone();
            let refspecs = refspecs.clone();
            threads.push(thread::spawn(move || {
                let proxy = match has_proxy {
                    true => Some(proxy_string_thread.as_str()),
                    false => None,
                };
                Self::sync_for_domain(
                    repos, refspecs, max_threads, hold, proxy)
            }));
        }
        threading::wait_remaining(threads, "syncing git repo groups")
    }
}