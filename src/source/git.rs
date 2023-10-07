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
        Tree, AutotagOption, FetchPrune, ErrorClass, ErrorCode, BranchType,
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

const _REFSPECS_MASTER_ONLY: &[&str] = 
    &["+refs/heads/master:refs/heads/master"];

// #[derive(Clone)]
// pub(crate) enum Refspecs {
//     HeadsTags,
//     MasterOnly
// }

// impl Refspecs {
//     fn get(&self) -> &[&str] {
//         match self {
//             Refspecs::HeadsTags => REFSPECS_HEADS_TAGS,
//             Refspecs::MasterOnly => REFSPECS_MASTER_ONLY,
//         }
//     }
// }

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
    branches: Vec<String>,
}

pub(crate) trait ToReposMap {
    fn branch(&self) -> Option<String>;
    fn url(&self) -> &str;
    fn hash_url(&self) -> u64;
    fn path(&self) -> Option<&Path>;
    fn to_repo(&self, parent: &str, gmr: Option<&Gmr>, branch: Option<String>) 
        -> Result<Repo, ()> 
    {
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
            Ok(mut repo) => {
                if let Some(branch) = branch {
                    repo.branches.push(branch)
                }
                Ok(repo)
            },
            Err(()) => {
                eprintln!(
                    "Failed to open bare repo for git source '{}'",
                    url);
                Err(())
            },
        }
    }

    fn to_repos_map(
        map: HashMap<u64, Vec<Self>>, parent: &str, gmr: Option<&Gmr>
    ) -> Result<HashMap<u64, Vec<Repo>>, ()>
    where Self: Sized
    {
        let mut repos_map = HashMap::new();
        for (domain, sources) in map {
            let mut repos: Vec<Repo> = vec![];
            for source in sources.iter() {
                let source_url = source.url();
                let mut existing = None;
                for repo in repos.iter_mut() {
                    if repo.url == source_url {
                        existing = Some(repo)
                    }
                }
                let new_branch = source.branch();
                if let Some(existing) = existing {
                    let existing_branches = 
                        &mut existing.branches;
                    if existing_branches.len() == 0 {
                        continue
                    }
                    let new_branch = match new_branch {
                        Some(branch) => branch,
                        None => {
                            existing_branches.clear();
                            continue
                        },
                    };
                    let mut branch_found = false;
                    for branch in existing_branches.iter() {
                        if &new_branch == branch {
                            branch_found = true;
                            break
                        }
                    }
                    if branch_found {
                        continue
                    }
                    existing_branches.push(new_branch);
                    continue
                }
                match source.to_repo(parent, gmr, new_branch) {
                    Ok(repo) => repos.push(repo),
                    Err(()) => {
                        return Err(())
                    },
                }
            }
            match repos_map.insert(domain, repos) {
                Some(_) => {
                    eprintln!("Duplicated key for repos map");
                    return Err(())
                },
                None => (),
            }
        }
        Ok(repos_map)
    }
}

impl ToReposMap for super::Source {
    
    fn url(&self) -> &str {
        self.url.as_str()
    }

    fn hash_url(&self) -> u64 {
        self.hash_url
    }

    fn path(&self) -> Option<&Path> {
        None
    }

    fn branch(&self) -> Option<String> {
        None
    }
}

pub(super) fn push_source(sources: &mut Vec<super::Source>, source: &super::Source) {
    for source_cmp in sources.iter() {
        if source.hash_url == source_cmp.hash_url {
            return
        }
    }
    sources.push(source.clone())
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
    fetch_opts.download_tags(AutotagOption::All)
        .prune(FetchPrune::On)
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
    refspecs: &[&str],
    tries: u8
) -> Result<(), ()> 
{
    for _ in 0..tries {
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
            for _ in 0..tries {
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
            eprintln!("Failed to fetch from remote '{}' after {} retries",
                remote_safe_url(&remote), tries);
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
                std::fs::remove_dir_all(&self.path).or(Err(()))
            }
        }
    }

    fn init_bare<P: AsRef<Path>>(path: P, url: &str, gmr: Option<&Gmr>)
        -> Result<Self, ()> 
    {
        match Repository::init_bare(&path) {
            Ok(repo) => {
                let repo = Self {
                    path: path.as_ref().to_owned(),
                    url: url.to_owned(),
                    mirror: optional_gmr(gmr, url),
                    repo,
                    branches: vec![],
                };
                match repo.add_remote() {
                    Ok(_) => Ok(repo),
                    Err(_) => Err(()),
                }
            },
            Err(e) => {
                eprintln!("Failed to create {}: {}",
                            &path.as_ref().display(), e);
                Err(())
            }
        }
    }

    pub(crate) fn open_bare<P: AsRef<Path>>(
        path: P, url: &str, gmr: Option<&Gmr>
    ) -> Result<Self, ()>
    {
        match Repository::open_bare(&path) {
            Ok(repo) => Ok(Self {
                path: path.as_ref().to_owned(),
                url: url.to_owned(),
                mirror: optional_gmr(gmr, url),
                repo,
                branches: vec![],
            }),
            Err(e) => {
                if e.class() == ErrorClass::Os &&
                e.code() == ErrorCode::NotFound {
                    Self::init_bare(path, url, gmr)
                } else {
                    eprintln!("Failed to open {}: {}",
                            path.as_ref().display(), e);
                    Err(())
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
                break
            }
        }
        Ok(())
    }

    fn _update_head(&self, remote: &mut Remote) -> Result<(), ()> {
        Self::update_head_raw(&self.repo, remote)
    }

    fn sync_raw(
        repo: &Repository, url: &str, proxy: Option<&str>, refspecs: &[&str], tries: u8
    ) -> Result<(), ()> 
    {
        let mut remote =
            repo.remote_anonymous(url).or(Err(()))?;
        let mut fetch_opts = fetch_opts_init();
        fetch_remote(&mut remote, &mut fetch_opts, proxy, refspecs, tries)?;
        Self::update_head_raw(repo, &mut remote)?;
        Ok(())
    }

    pub(crate) fn sync(&self, proxy: Option<&str>)
        -> Result<(), ()>
    {
        let mut refspecs_dynamic = vec![];
        let mut refspecs_ref = vec![];
        let mut refspecs = REFSPECS_HEADS_TAGS;
        if self.branches.len() > 0 {
            for branch in self.branches.iter() {
                refspecs_dynamic.push(
                    format!("+refs/heads/{}:refs/heads/{}", branch, branch));
            }
            for refspec in refspecs_dynamic.iter() {
                refspecs_ref.push(refspec.as_str())
            }
            refspecs = refspecs_ref.as_slice()
        }
        if let Some(mirror) = &self.mirror {
            println!("Syncing repo '{}' with gmr '{}' before actual remote",
                        &self.path.display(), &mirror);
            if let Ok(_) = Self::sync_raw(
                &self.repo, &mirror, None, refspecs, 1
            ) {
                return Ok(())
            }
        }
        println!("Syncing repo '{}' with '{}' ", 
            &self.path.display(), &self.url);
        Self::sync_raw(&self.repo, &self.url, proxy, refspecs, 3)
    }

    fn get_branch<'a>(&'a self, branch: &str) -> Result<Branch<'a>, ()> {
        match self.repo.find_branch(branch, BranchType::Local) {
            Ok(branch) => Ok(branch),
            Err(e) => {
                eprintln!("Failed to find branch '{}': {}", branch, e);
                Err(())
            }
        }
    }

    fn get_branch_commit<'a>(&'a self, branch: &str) -> Result<Commit<'a>, ()> {
        let branch_gref = self.get_branch(branch)?;
        match branch_gref.get().peel_to_commit() {
            Ok(commit) => Ok(commit),
            Err(e) => {
                eprintln!("Failed to peel branch '{}' to commit: {}", branch, e);
                return Err(())
            },
        }
    }

    pub(crate) fn _get_branch_commit_id(&self, branch: &str) -> Result<Oid, ()> {
        Ok(self.get_branch_commit(branch)?.id())
    }

    fn get_commit_tree<'a>(&'a self, commit: &Commit<'a>, subtree: Option<&Path>
    )   -> Result<Tree<'a>, ()> 
    {
        let tree = commit.tree().or_else(|e| {
            eprintln!("Failed to get tree pointed by commit: {}", e);
            Err(())
        })?;
        let subtree = match subtree {
            Some(subtree) => subtree,
            None => return Ok(tree),
        };
        let entry = match tree.get_path(subtree) {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!("Failed to get sub tree: {}", e);
                return Err(())
            },
        };
        Ok(entry.to_object(&self.repo).or_else(|e|{
            eprintln!("Failed to convert entry to object: {}", e);
            Err(())
        })?
        .as_tree()
        .ok_or_else(||{
            eprintln!("Failed to convert object ot ree")})?
        .to_owned())
    }

    fn get_branch_tree<'a>(&'a self, branch: &str, subtree: Option<&Path>) 
        -> Result<Tree<'a>, ()> 
    {
        let commit = self.get_branch_commit(branch)?;
        self.get_commit_tree(&commit, subtree)
    }

    pub(crate) fn get_branch_commit_or_subtree_id(&self, 
        branch: &str, subtree: Option<&Path>
    ) -> Result<Oid, ()> 
    {
        let commit = self.get_branch_commit(branch)?;
        if let None = subtree {
            return Ok(commit.id())
        }
        let tree = self.get_commit_tree(&commit, subtree)?;
        Ok(tree.id())
    }

    fn get_tree_entry_blob<'a>(&'a self, tree: &Tree, name: &str)
        -> Result<Blob<'a>, ()>
    {
        let entry =
            match tree.get_name(name) {
                Some(entry) => entry,
                None => {
                    eprintln!("Failed to find entry of {}", name);
                    return Err(())
                },
            };
        let object =
            match entry.to_object(&self.repo) {
                Ok(object) => object,
                Err(e) => {
                    eprintln!("Failed to convert tree entry to object: {}", e);
                    return Err(())
                },
            };
        match object.into_blob() {
            Ok(blob) => Ok(blob),
            Err(_) => {
                eprintln!("Failed to convert into a blob");
                return Err(())
            },
        }
    }

    pub(crate) fn get_branch_entry_blob<'a>(&'a self, 
        branch: &str, subtree: Option<&Path>, name: &str
    )
        -> Result<Blob<'a>, ()>
    {
        let tree = self.get_branch_tree(branch, subtree)?;
        self.get_tree_entry_blob(&tree, name)
    }

    pub(crate) fn get_pkgbuild_blob(&self, branch: &str, subtree: Option<&Path>) 
        -> Result<Blob, ()> 
    {
        self.get_branch_entry_blob(branch, subtree, "PKGBUILD")
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

    pub(crate) fn checkout<P>(&self, target: P, branch: &str, subtree: Option<&Path>)
        -> Result<(),()>
    where
        P: AsRef<Path>
    {
        let tree = self.get_branch_tree(branch, subtree)?;
        self.repo.cleanup_state().or(Err(()))?;
        self.repo.set_workdir(
                    target.as_ref(),
                    false).or(Err(()))?;
        self.repo.checkout_tree(
                    tree.as_object(),
                    Some(CheckoutBuilder::new().force()))
                    .or(Err(()))
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
            repos.last().ok_or(())?.get_domain());
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
            threads.push(thread::spawn(move ||{
                let proxy = match has_proxy {
                    true => Some(proxy_string_thread.as_str()),
                    false => None,
                };
                repo.sync(proxy)
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
                        repos.last().ok_or(())?.get_domain());
            let proxy_string_thread = proxy_string.clone();
            threads.push(thread::spawn(move || {
                let proxy = match has_proxy {
                    true => Some(proxy_string_thread.as_str()),
                    false => None,
                };
                Self::sync_for_domain(
                    repos,max_threads, hold, proxy)
            }));
        }
        threading::wait_remaining(threads, "syncing git repo groups")
    }
}