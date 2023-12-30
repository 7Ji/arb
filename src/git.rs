// Todo: use `gitoxide` instead of `git2-rs`, for memory safety

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
use xxhash_rust::xxh3::xxh3_64;
// use url::Url;
use std::{
        collections::HashMap,
        fs::metadata,
        io::Write,
        path::{
            Path,
            PathBuf
        },
        os::unix::fs::MetadataExt,
        str::FromStr,
        thread::{self, spawn, JoinHandle}, borrow::Cow, fmt::format,
    };

use crate::{Error, Result, proxy::{Proxy, NOPROXY}, pkgbuild::{Pkgbuild, Pkgbuilds}, aur::AurResult};

const REFSPECS_HEADS_TAGS: &[&str] = &[
    "+refs/heads/*:refs/heads/*",
    "+refs/tags/*:refs/tags/*"
];

struct Gmr<'a> {
    prefix: &'a str,
}

impl<'a> From<&'a str> for Gmr<'a> {
    fn from(value: &'a str) -> Self {
        Self {
            prefix: value
        }
    }
}

impl<'a> Gmr<'a> {
    fn convert(&self, orig: &str) -> String {
        let orig_url = match Url::from_str(orig) {
            Ok(url) => url,
            Err(e) => {
                log::warn!("Cannot parse original URL '{}': {}", orig, e);
                return Default::default()
            },
        };
        let host_str = match orig_url.host_str() {
            Some(host) => host,
            None => {
                log::warn!("Cannot get host str from original URL '{}'", orig);
                return Default::default()
            },
        };
        format!("{}/{}{}", self.prefix, host_str, orig_url.path())
    }

    fn once(gmr: &str, orig: &str) -> String {
        Self::from(gmr).convert(orig)
    }
}

pub(crate) struct Repo {
    pub(crate) path: PathBuf,
    pub(crate) url: String,
    pub(crate) repo: Repository,
    pub(crate) branches: Vec<String>,
}

impl TryFrom<&Pkgbuild> for Repo {
    type Error = Error;

    fn try_from(pkgbuild: &Pkgbuild) -> Result<Self> {
        if pkgbuild.branch.is_empty() {
            log::error!("PKGBUILD has empty branch");
            return Err(Error::InvalidConfig)
        }
        let path = format!("sources/PKGBUILD/{:016x}", 
            xxh3_64(pkgbuild.url.as_bytes()));
        let mut repo = Self::open_bare_init_if_non_existing(
            path, &pkgbuild.url)?;
        repo.branches.push(pkgbuild.branch);
        Ok(repo)
    }
}

type ReposVec = Vec<Repo>;

pub(crate) struct ReposList {
    pub(crate) entries: ReposVec
}

impl From<Vec<Repo>> for ReposList {
    fn from(entries: Vec<Repo>) -> Self {
        Self {entries}
    }
}

impl Into<Vec<Repo>> for ReposList {
    fn into(self) -> Vec<Repo> {
        self.entries
    }
}

type ReposHashMap = HashMap<u64, ReposList>;

#[derive(Default)]
pub(crate) struct ReposMap {
    pub(crate) map: ReposHashMap
}

impl TryFrom<&Pkgbuilds> for ReposMap {
    type Error = Error;

    fn try_from(pkgbuilds: &Pkgbuilds) -> Result<Self> {
        let mut map = ReposHashMap::new();
        for pkgbuild in pkgbuilds.entries.iter() {
            let repo = match Repo::try_from(pkgbuild) {
                Ok(repo) => repo,
                Err(e) => {
                    log::error!("Failed to convert PKGBUILD to Repo when \
                        trying to conver PKGBUILDs to ReposMap");
                    return Err(e.into())
                },
            };
            let key = match Url::parse(&pkgbuild.url) {
                Ok(url) => if let Some(domain) = url.domain() {
                    xxh3_64(domain.as_bytes())
                } else {
                    log::warn!("PKGBUILD URL '{}' does not have domain",
                        &pkgbuild.url);
                    0
                },
                Err(e) => {
                    log::warn!("Failed to parse PKGBUILD URL '{}': {}", 
                            &pkgbuild.url, e);
                    0
                },
            };
            match map.get_mut(&key) {
                Some(list) => list.entries.push(repo),
                None => if let Some(list) = 
                        map.insert(key, vec![repo].into()) 
                    {
                        log::error!("Impossible key {} already in map", key);
                        return Err(Error::ImpossibleLogic)
                    },
            }
        }
        Ok(Self{map})
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

fn fetch_opts_init<'a>(terminal: bool) -> FetchOptions<'a> {
    let mut cbs = RemoteCallbacks::new();
    if terminal {
        cbs.sideband_progress(|log| {
                print!("Remote: {}", String::from_utf8_lossy(log));
                true
            });
        cbs.transfer_progress(gcb_transfer_progress);
    }
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
    proxy: &Proxy,
    refspecs: &[&str],
    tries: usize
) -> Result<()>
{
    let (tries_without_proxy, tries_with_proxy) = 
        proxy.get_tries(tries);
    let mut last_error = Error::ImpossibleLogic;
    for _ in 0..tries_without_proxy {
        match remote.fetch(
            refspecs, Some(fetch_opts), None
        ) {
            Ok(_) => return Ok(()),
            Err(e) => {
                log::error!("Failed to fetch from remote '{}': {}",
                    remote_safe_url(&remote), e);
                last_error = e.into();
            },
        }
    }
    if proxy.url.is_empty() {
        log::error!("Failed to fetch from remote '{}' after {} tries and \
            there's no proxy to retry, giving up", remote_safe_url(&remote),
            tries_without_proxy);
        return Err(last_error)
    }
    if tries_without_proxy > 0 {
        log::warn!("Failed to fetch from remote '{}' after {} tries, will use \
            proxy to retry", remote_safe_url(&remote), tries_without_proxy);
    }
    let mut proxy_opts = ProxyOptions::new();
    proxy_opts.url(&proxy.url);
    fetch_opts.proxy_options(proxy_opts);
    for _ in 0..tries_with_proxy {
        match remote.fetch(
            refspecs, Some(fetch_opts), None) {
            Ok(_) => return Ok(()),
            Err(e) => {
                log::error!("Failed to fetch from remote '{}': {}",
                    remote_safe_url(&remote), e);
                last_error = e.into()
            },
        }
    };
    log::error!("Failed to fetch from remote '{}' even with proxy",
        remote_safe_url(&remote));
    Err(last_error)
}

impl Repo {
    fn add_remote(&self) -> Result<()> {
        match self.repo.remote_with_fetch(
            "origin", &self.url, "+refs/*:refs/*") {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Failed to add remote {}: {}",
                            self.path.display(), e);
                if let Err(e) = std::fs::remove_dir_all(&self.path) {
                    return Err(e.into())
                }
                Err(e.into())
            }
        }
    }

    fn init_bare<P: AsRef<Path>>(path: P, url: &str)
        -> Result<Self>
    {
        match Repository::init_bare(&path) {
            Ok(repo) => {
                let repo = Self {
                    path: path.as_ref().to_owned(),
                    url: url.to_owned(),
                    repo,
                    branches: vec![],
                };
                repo.add_remote().and(Ok(repo))
            },
            Err(e) => {
                log::error!("Failed to create {}: {}",
                            &path.as_ref().display(), e);
                Err(e.into())
            }
        }
    }

    pub(crate) fn open_bare_init_if_non_existing<P: AsRef<Path>>(
        path: P, url: &str) -> Result<Self>
    {
        match Repository::open_bare(&path) {
            Ok(repo) => Ok(Self {
                path: path.as_ref().to_owned(),
                url: url.to_owned(),
                repo,
                branches: vec![],
            }),
            Err(e) => {
                if e.class() == ErrorClass::Os &&
                e.code() == ErrorCode::NotFound {
                    Self::init_bare(path, url)
                } else {
                    log::error!("Failed to open {}: {}",
                            path.as_ref().display(), e);
                    Err(e.into())
                }
            },
        }
    }

    fn update_head_raw(repo: &Repository, remote: &mut Remote)
        -> Result<()>
    {
        let url = remote_safe_url(remote);
        let heads = match remote.list() {
            Ok(heads) => heads,
            Err(e) => {
                log::error!("Failed to list remote '{}' for repo '{}': {}",
                    url, repo.path().display(), e);
                return Err(e.into())
            },
        };
        for head in heads {
            if head.name() == "HEAD" {
                if let Some(target) = head.symref_target() {
                    match repo.set_head(target) {
                        Ok(_) => return Ok(()),
                        Err(e) => {
                            log::warn!("Failed to set head for '{}': {}",
                                        url, e);
                        },
                    }
                }
                break
            }
        }
        Ok(())
    }

    fn sync_raw(
        repo: &Repository, url: &str, proxy: &Proxy, refspecs: &[&str],
        tries: usize, terminal: bool
    ) -> Result<()>
    {
        let mut remote =
            repo.remote_anonymous(url).map_err(Error::from)?;
        let mut fetch_opts = fetch_opts_init(terminal);
        fetch_remote(&mut remote, &mut fetch_opts, proxy, refspecs, tries)?;
        Self::update_head_raw(repo, &mut remote)?;
        Ok(())
    }

    pub(crate) fn sync(&self, proxy: &Proxy, terminal: bool)
        -> Result<()>
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
            log::info!("Syncing repo '{}' with gmr '{}' before actual remote",
                        &self.path.display(), &mirror);
            if let Ok(_) = Self::sync_raw(
                &self.repo, &mirror, &NOPROXY, refspecs, 1, terminal
            ) {
                return Ok(())
            }
        }
        log::info!("Syncing repo '{}' with '{}' ",
            &self.path.display(), &self.url);
        Self::sync_raw(&self.repo, &self.url, proxy, refspecs, 3, terminal)
    }

    fn get_branch<'a>(&'a self, branch: &str) -> Result<Branch<'a>> {
        match self.repo.find_branch(branch, BranchType::Local) {
            Ok(branch) => Ok(branch),
            Err(e) => {
                log::error!("Failed to find branch '{}': {}", branch, e);
                Err(e.into())
            }
        }
    }

    fn get_branch_commit<'a>(&'a self, branch: &str) -> Result<Commit<'a>> {
        let branch_gref = self.get_branch(branch)?;
        match branch_gref.get().peel_to_commit() {
            Ok(commit) => Ok(commit),
            Err(e) => {
                log::error!("Failed to peel branch '{}' to commit: {}", branch, e);
                return Err(e.into())
            },
        }
    }

    pub(crate) fn _get_branch_commit_id(&self, branch: &str) -> Result<Oid> {
        Ok(self.get_branch_commit(branch)?.id())
    }

    fn get_commit_tree<'a>(&'a self, commit: &Commit<'a>, subtree: Option<&Path>
    )   -> Result<Tree<'a>>
    {
        let tree = match commit.tree() {
            Ok(tree) => tree,
            Err(e) => {
                log::error!("Failed to get tree pointed by commit: {}", e);
                return Err(e.into())
            },
        };
        let subtree = match subtree {
            Some(subtree) => subtree,
            None => return Ok(tree),
        };
        let entry = match tree.get_path(subtree) {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("Failed to get sub tree: {}", e);
                return Err(e.into())
            },
        };
        let object = match entry.to_object(&self.repo) {
            Ok(object) => object,
            Err(e) => {
                log::error!("Failed to convert tree entry to object: {}", e);
                return Err(e.into())
            },
        };
        match object.as_tree() {
            Some(tree) => Ok(tree.to_owned()),
            None => {
                log::error!("Not a subtree : '{}'", subtree.display());
                Err(Error::GitObjectMissing)
            },
        }
    }

    fn get_branch_tree<'a>(&'a self, branch: &str, subtree: Option<&Path>)
        -> Result<Tree<'a>>
    {
        let commit = self.get_branch_commit(branch)?;
        self.get_commit_tree(&commit, subtree)
    }

    pub(crate) fn get_branch_commit_or_subtree_id(&self,
        branch: &str, subtree: Option<&Path>
    ) -> Result<Oid>
    {
        let commit = self.get_branch_commit(branch)?;
        if let None = subtree {
            return Ok(commit.id())
        }
        Ok(self.get_commit_tree(&commit, subtree)?.id())
    }

    fn get_tree_entry_blob<'a>(&'a self, tree: &Tree, name: &str)
        -> Result<Blob<'a>>
    {
        let entry =
            match tree.get_name(name) {
                Some(entry) => entry,
                None => {
                    log::error!("Failed to find entry of {}", name);
                    return Err(Error::GitObjectMissing)
                },
            };
        let object =
            match entry.to_object(&self.repo) {
                Ok(object) => object,
                Err(e) => {
                    log::error!("Failed to convert tree entry to object: {}", e);
                    return Err(e.into())
                },
            };
        match object.into_blob() {
            Ok(blob) => Ok(blob),
            Err(object) => {
                log::error!("Failed to convert object '{}' into a blob", object.id());
                return Err(Error::GitObjectMissing)
            },
        }
    }

    pub(crate) fn get_branch_entry_blob<'a>(&'a self,
        branch: &str, subtree: Option<&Path>, name: &str
    )
        -> Result<Blob<'a>>
    {
        let tree = self.get_branch_tree(branch, subtree)?;
        self.get_tree_entry_blob(&tree, name)
    }

    pub(crate) fn get_pkgbuild_blob(&self, branch: &str, subtree: Option<&Path>)
        -> Result<Blob>
    {
        self.get_branch_entry_blob(branch, subtree, "PKGBUILD")
    }

    pub(crate) fn healthy(&self) -> bool {
        let head = match self.repo.head() {
            Ok(head) => head,
            Err(e) => {
                log::error!("Failed to get head of repo '{}': {}",
                        self.path.display(), e);
                return false
            },
        };
        return match head.peel_to_commit() {
            Ok(_) => true,
            Err(e) => {
                log::error!("Failed to get head of repo '{}': {}",
                        self.path.display(), e);
                false
            },
        };
    }

    pub(crate) fn checkout<P>(&self, target: P, branch: &str, subtree: Option<&Path>)
        -> Result<()>
    where
        P: AsRef<Path>
    {
        let tree = self.get_branch_tree(branch, subtree)?;
        self.repo.cleanup_state().map_err(Error::from)?;
        self.repo.set_workdir(
                    target.as_ref(),
                    false)
                .map_err(Error::from)?;
        self.repo.checkout_tree(
                    tree.as_object(),
                    Some(CheckoutBuilder::new().force()))
                .map_err(Error::from)
    }

    fn get_domain(&self) -> String {
        if let Ok(url) = Url::parse(&self.url) {
            if let Some(domain) = url.domain() {
                return domain.to_string()
            }
        }
        format!("of url {}", &self.url)
    }

    

    fn last_fetch(&self) -> i64 {
        let path_fetch_head = self.path.join("FETCH_HEAD");
        let metadata = match metadata(&path_fetch_head) {
            Ok(metadata) => metadata,
            Err(e) => {
                log::error!("Failed to get metadata of fetch time, \
                    consider 0: {}", e);
                return 0
            },
        };
        metadata.mtime()
    }
}

impl ReposList {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn filter_aur(&mut self) -> Result<()> {
        let mut pkgs: Vec<String> = Vec::new();
        for repo in self.entries.iter() {
            let url = match Url::parse(&repo.url) {
                Ok(url) => url,
                Err(e) => {
                    log::error!("Failed to parse AUR url '{}': {}", &repo.url, e);
                    return Err(e.into());
                },
            };
            let pkg = url.path()
                .trim_start_matches('/').trim_end_matches(".git");
            pkgs.push(pkg.into());
        }
        if pkgs.len() != self.len() {
            log::error!("Pkgs and repos len mismatch");
            return Err(Error::ImpossibleLogic)
        }
        let mut aur_result = match AurResult::from_pkgs(&pkgs) {
            Ok(aur_result) => aur_result,
            Err(e) => {
                log::error!("Failed to get result from AUR RPC");
                return Err(e)
            },
        };
        if aur_result.results.len() == self.len() {
            let mut i = 0;
            while i < self.len() {
                let repo = match self.entries.get(i) {
                    Some(repo) => repo,
                    None => {
                        log::error!("Failed to get repo");
                        return Err(Error::ImpossibleLogic)
                    },
                };
                let pkg = match aur_result.results.get(i) {
                    Some(pkg) => pkg,
                    None => {
                        log::error!("Failed to get pkg");
                        return Err(Error::ImpossibleLogic)
                    },
                };
                // leave a 1-min window
                if repo.last_fetch() > pkg.last_modified + 60 {
                    log::info!(
                        "Repo '{}' last fetch later than AUR last modified, \
                        skippping it", &repo.url);
                    self.entries.swap_remove(i);
                    aur_result.results.swap_remove(i);
                } else {
                    log::info!("Repo '{}' needs update from AUR",
                        &repo.url);
                    i += 1
                }
            }
        } else {
            aur_result.results.sort_unstable_by(
                |result_a, result_b|
                    result_a.name.cmp(&result_b.name));
            let mut i = 0;
            while i < self.len() {
                let repo = match self.entries.get(i) {
                    Some(repo) => repo,
                    None => {
                        log::error!("Failed to get repo");
                        return Err(Error::ImpossibleLogic)
                    },
                };
                let pkg = match pkgs.get(i) {
                    Some(pkg) => pkg,
                    None => {
                        log::error!("Failed to get pkg");
                        return Err(Error::ImpossibleLogic)
                    },
                };
                if let Ok(j) = aur_result.results.binary_search_by(
                    |result|result.name.cmp(pkg))
                {
                    let result = match
                        aur_result.results.get(j)
                    {
                        Some(result) => result,
                        None => {
                            log::error!("Failed to get result");
                            return Err(Error::ImpossibleLogic)
                        },
                    };
                    if repo.last_fetch() > result.last_modified + 60 {
                        log::info!("Repo '{}' last fetch later than AUR last \
                            modified, skippping it", &repo.url);
                        self.entries.swap_remove(i);
                        pkgs.swap_remove(i);
                    } else {
                        log::info!("Repo '{}' needs update from AUR",
                            &repo.url);
                        i += 1
                    }
                } else { // Can not find
                    log::info!("Repo '{}' not found, needs update from AUR",
                        &repo.url);
                    i += 1
                }
            }
        }
        log::info!("Filtered AUR repos");
        Ok(())
    }

    fn sync_for_domain_mt(
        repos: Vec<Self>,
        max_threads: usize,
        hold: bool,
        proxy: Option<&Proxy>,
        terminal: bool
    ) -> Result<()>
    {
        // let pool = ThreadPool::new(max_threads,
        //     format!("syncing git repos from domain '{}'",
        //         repos.last().ok_or(())?.get_domain()));
        // let mut r = 
        let mut threads = Vec::new();
        let job = match repos.last() {
            Some(repo) => format!("syncing git repos from domain '{}'", 
                repo.get_domain()),
            None => return Ok(()),
        };
        let mut r = Ok(());
        for repo in repos {
            if hold {
                if repo.healthy() {
                    continue;
                } else {
                    log::info!(
                        "Holdgit set but repo '{}' not healthy, need update",
                        repo.path.display());
                }
            }
            let proxy_thread = proxy.and_then(
                |proxy|Some(proxy.to_owned()));
            // match pool.push(move ||
            //         repo.sync(proxy_thread.as_ref(), terminal)) {
            //             Ok(_) => todo!(),
            //             Err(_) => todo!(),
            //         }

            if let Err(e) =
                threading::wait_if_too_busy(&mut threads, max_threads, &job) {
                r = Err(e);
            }
            threads.push(thread::spawn(move ||{
                repo.sync(proxy_thread.as_ref(), terminal)
            }));
        }
        if let Err(e) = threading::wait_remaining(threads, &job) {
            r = Err(e)
        }
        r
    }

    fn sync_for_domain_st(
        repos: Vec<Self>,
        hold: bool,
        proxy: Option<&Proxy>,
        terminal: bool
    ) -> Result<()>
    {
        let mut r = Ok(());
        for repo in repos {
            if hold {
                if repo.healthy() {
                    continue;
                } else {
                    log::info!(
                        "Holdgit set but repo '{}' not healthy, need update",
                        repo.path.display());
                }
            }
            if let Err(e) = repo.sync(proxy, terminal) {
                log::error!("Failed to sync repo '{}'", &repo.url);
                r = Err(e);
            }
        }
        r
    }

    fn sync_for_domain(
        repos: Vec<Self>,
        max_threads: usize,
        hold: bool,
        proxy: Option<&Proxy>,
        terminal: bool
    ) -> Result<()>
    {
        if max_threads >= 2 && repos.len() >= 2 {
            Self::sync_for_domain_mt(repos, max_threads, hold, proxy, terminal)
        } else {
            Self::sync_for_domain_st(repos, hold, proxy, terminal)
        }
    }

    fn sync_for_aur(
        mut repos: Vec<Self>,
        hold: bool,
        proxy: Option<&Proxy>,
        terminal: bool
    ) -> Result<()>
    {
        if Self::filter_aur(&mut repos).is_err() {
            log::error!("Warning: failed to filter AUR repos")
        }
        if repos.is_empty() {
            return Ok(())
        }
        Self::sync_for_domain(repos, 1, hold, proxy, terminal)
    }

    fn sync_generic(&mut self, gmr: &str, holdpkg: bool, proxy: &Proxy) -> Result<()> {
        if self.entries.is_empty() { return Ok(()) }
        Ok(())
    }


    fn sync_aur(&mut self, gmr: &str, holdpkg: bool, proxy: &Proxy) -> Result<()> {
        if self.entries.is_empty() { return Ok(()) }
        self.filter_aur();
        if self.entries.is_empty() { return Ok(()) }
        Ok(())
    }
}

impl ReposMap {
    pub(crate) fn sync_mt(self, gmr: &str, holdpkg: bool, proxy: &Proxy) 
        -> Result<()> 
    {
        let mut threads = Vec::new();
        for (key, list) in self.map {
            if list.entries.is_empty() { continue }
            let gmr = gmr.to_owned();
            let proxy = proxy.clone();
            let thread = spawn(move ||
                if key == 0xb463cbdec08d6265 {
                    list.sync_aur(&gmr, holdpkg, &proxy)
                } else {
                    list.sync_generic(&gmr, holdpkg, &proxy)
                });
            threads.push(thread)
        }
        let mut r = Ok(());
        for thread in threads {
            match thread.join() {
                Ok(tr) => if let Err(e) = tr {
                    log::error!("A git sync thread has bad return");
                    r = tr
                },
                Err(e) => {
                    // Prefer non-thread error error
                    if r.is_ok() {
                        log::error!("A git sync thread has panicked");
                        r = Err(e.into())
                    }
                },
            }
        }
        r
    }
}