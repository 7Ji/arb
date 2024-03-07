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
// use threadpool::ThreadPool;
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
        thread::{self, spawn},
    };


use crate::{aur::AurResult, pkgbuild::{Pkgbuild, Pkgbuilds}, proxy::{Proxy, NOPROXY}, threading, Error, Result};

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

    /// Similar to Gmr::convert(), but without an instance to operator on
    fn convert_oneshot(gmr: &'a str, orig: &str) -> String {
        Self::from(gmr).convert(orig)
    }
}

pub(crate) struct Repo {
    pub(crate) path: PathBuf,
    pub(crate) url: String,
    pub(crate) repo: Repository,
    pub(crate) refspecs: Vec<String>,
}

fn refspec_same_branch(branch: &str) -> String {
    format!("+refs/heads/{}:refs/heads/{}", branch, branch)
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
        repo.refspecs.push(refspec_same_branch(&pkgbuild.branch));
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
    if progress.received_objects() == progress.total_objects() {
        print!(
            "Resolving deltas {}/{}\r",
            progress.indexed_deltas(),
            progress.total_deltas()
        );
    } else {
        let network_pct =
            (100 * progress.received_objects()) / progress.total_objects();
        let index_pct =
            (100 * progress.indexed_objects()) / progress.total_objects();
        let kbytes = progress.received_bytes() / 1024;
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
    crate::io::flush_stdout().is_ok()
}

fn gcb_sideband_progress(log: &[u8]) -> bool {
    print!("Remote: ", );
    crate::io::write_all_to_stdout(log).is_ok()
}

fn fetch_opts_init<'a>() -> FetchOptions<'a> {
    let mut cbs = RemoteCallbacks::new();
    if crate::io::is_stdout_terminal() {
        cbs.sideband_progress(gcb_sideband_progress);
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

fn fetch_remote<Str> (
    remote: &mut Remote,
    fetch_opts: &mut FetchOptions,
    proxy: &Proxy,
    refspecs: &[Str],
    tries: usize
) -> Result<()>
where
    Str: AsRef<str> + git2::IntoCString + Clone,
{
    let (tries_without_proxy, tries_with_proxy) = 
        proxy.tries_without_and_with(tries);
    let mut last_error = Error::ImpossibleLogic;
    for _ in 0..tries_without_proxy {
        if let Err(e)=  remote.fetch(
            refspecs, Some(fetch_opts), None
        ) {
            log::error!("Failed to fetch from remote '{}': {}",
                remote_safe_url(&remote), e);
            last_error = e.into();
        } else {
            return Ok(())
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
        if let Err(e) = remote.fetch(
            refspecs, Some(fetch_opts), None
        ) {
            log::error!("Failed to fetch from remote '{}': {}",
                remote_safe_url(&remote), e);
            last_error = e.into()
        } else {
            return Ok(())
        }
    }
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
                    refspecs: Default::default(),
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
                refspecs: Default::default(),
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
        let heads = match remote.list() {
            Ok(heads) => heads,
            Err(e) => {
                log::error!("Failed to list remote '{}' for repo '{}': {}",
                remote_safe_url(remote), repo.path().display(), e);
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
                                remote_safe_url(remote), e);
                        },
                    }
                }
                break
            }
        }
        Ok(())
    }

    fn sync_raw<Str> (
        repo: &Repository, url: &str, proxy: &Proxy, refspecs: &[Str],
        tries: usize
    ) -> Result<()>
    where
        Str: AsRef<str> + git2::IntoCString + Clone
    {
        let mut remote = match repo.remote_anonymous(url) {
            Ok(remote) => remote,
            Err(e) => {
                log::error!("Failed to create anonymous remote '{}': {}",
                                url, e);
                return Err(e.into())
            },
        };
        let mut fetch_opts = fetch_opts_init();
        fetch_remote(&mut remote, &mut fetch_opts, proxy, refspecs, tries)?;
        Self::update_head_raw(repo, &mut remote)?;
        Ok(())
    }

    fn sync_with_refspecs<Str>(&self, gmr: &str, proxy: &Proxy, refspecs: &[Str]
    ) -> Result<()>
    where
        Str: AsRef<str> + git2::IntoCString + Clone
    {
        if ! gmr.is_empty() {
            log::info!("Syncing repo '{}' with gmr '{}' before actual remote",
                        &self.path.display(), &gmr);
            if Self::sync_raw(&self.repo, 
                &Gmr::convert_oneshot(gmr, &self.url), &NOPROXY, 
                    refspecs, 1).is_ok() 
            { 
                return Ok(()) 
            }
        }
        log::info!("Syncing repo '{}' with '{}' ",
                        &self.path.display(), &self.url);
        Self::sync_raw(&self.repo, &self.url, proxy, refspecs, 3)
    }

    fn sync(&self, gmr: &str, proxy: &Proxy) -> Result<()>
    {
        if self.refspecs.is_empty() {
            self.sync_with_refspecs(gmr, proxy, REFSPECS_HEADS_TAGS)
        } else {
            self.sync_with_refspecs(gmr, proxy, &self.refspecs)
        }
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
                log::error!("Failed to peel branch '{}' to commit: {}", 
                            branch, e);
                return Err(e.into())
            },
        }
    }

    /// Get a tree pointed by a commit, optionally a subtree of the commit
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
                    log::error!("Failed to convert tree entry to object: {}",
                                     e);
                    return Err(e.into())
                },
            };
        match object.into_blob() {
            Ok(blob) => Ok(blob),
            Err(object) => {
                log::error!("Failed to convert object '{}' into a blob",
                             object.id());
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

    /// A shortcut to `get_branch_entry_blob(branch, subtree, "PKGBUILD")`
    pub(crate) fn get_branch_pkgbuild(
        &self, branch: &str, subtree: Option<&Path>
    ) -> Result<Blob>
    {
        self.get_branch_entry_blob(branch, subtree, "PKGBUILD")
    }

    /// Check if a repo's HEAD both exists and points to a valid commit
    pub(crate) fn is_head_healthy(&self) -> bool {
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

    /// Checkout a repo to `target`, from `branch` and optionally from `subtree`
    pub(crate) fn checkout<P>(
        &self, target: P, branch: &str, subtree: Option<&Path>
    ) -> Result<()>
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

    /// Get the repo's remote domain, e.g. for a repo with a remote URL 
    /// `https://github.com/7Ji/ampart.git`, the domain would be `github.com`.
    /// 
    /// This is `safe` in the sense it does not fail. If we really can't get the
    /// domain, this simply returns `of url [original url]`
    fn get_domain_safe(&self) -> String {
        if let Ok(url) = Url::parse(&self.url) {
            if let Some(domain) = url.domain() {
                return domain.to_string()
            }
        }
        format!("of url {}", &self.url)
    }

    /// Get the time we fetched from the remote last time, namely the `mtime` of
    /// file `FETCH_HEAD`
    fn get_last_fetch(&self) -> i64 {
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

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn filter_aur(&mut self) -> Result<()> {
        let mut pkgs: Vec<String> = Vec::new();
        for repo in self.entries.iter() {
            let url = match Url::parse(&repo.url) {
                Ok(url) => url,
                Err(e) => {
                    log::error!("Failed to parse AUR url '{}': {}", 
                        &repo.url, e);
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
        let aur_result = match AurResult::from_pkgs(&pkgs) {
            Ok(aur_result) => aur_result,
            Err(e) => {
                log::error!("Failed to get result from AUR RPC");
                return Err(e)
            },
        };
        if aur_result.len() != self.len() {
            log::error!("AUR results length mismatch repos len");
            return Err(Error::ImpossibleLogic)
        }
        for index in (0..self.len()).rev() {
            let mtime_repo = self.entries[index].get_last_fetch();
            let mtime_pkg = aur_result.results[index].last_modified;
            let (mtime_pkg_delayed, overflowed) = 
                mtime_pkg.overflowing_add(60);
            if overflowed || mtime_repo >= mtime_pkg_delayed{ 
                let repo = self.entries.swap_remove(index);
                log::info!("Repo '{}' last fetch later than AUR last modified, \
                    skippping it", &repo.url);
            }
        }
        log::info!("Filtered AUR repos");
        Ok(())
    }

    fn sync_single_thread(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        let mut r = Ok(());
        for repo in self.entries.iter_mut() {
            if let Err(e) = repo.sync(gmr, proxy) {
                r = Err(e)
            }
        }
        r
    }

    fn sync_multi_threaded(
        mut self, gmr: &str, proxy: &Proxy, hold: bool, max_threads: usize
    ) -> Result<()> 
    {
        if max_threads <= 1 {
            return self.sync_single_thread(gmr, proxy, hold)
        }
        let mut pool = match self.entries.first() {
            Some(repo) => 
                threading::ThreadPool::new(max_threads, 
                format!("syncing repos from domain '{}'", 
                                repo.get_domain_safe())),
            None => threading::ThreadPool::new(max_threads, 
                    "syncing repos"),
        };
        let mut r = Ok(());
        for repo in self.entries {
            let gmr = gmr.to_owned();
            let proxy = proxy.clone();
            match pool.push(move||repo.sync(&gmr, &proxy)) {
                Ok(lastr) => 
                    if let Some(lastr) = lastr {
                        if let Err(e) = lastr {
                            log::error!("A previous repo syncer failed: {}", e);
                            r = Err(e)
                        }
                    },
                Err(e) => {
                    log::error!("Failed to add repo syncing: {}", e);
                    r = Err(e)
                },
            }
        }
        let (_, lastr) = pool.wait_all_check();
        if let Err(e) = lastr {
            log::error!("A previous repo syncer failed: {}", e);
            r = Err(e)
        }
        r
    }

    fn sync_generic(self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        self.sync_multi_threaded(gmr, proxy, hold, 10)
    }


    fn sync_aur(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        self.filter_aur()?;
        if self.entries.is_empty() { return Ok(()) }
        self.sync_single_thread(gmr, proxy, hold)
    }
}

impl ReposMap {
    pub(crate) fn sync(self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        let mut threads = Vec::new();
        for (key, mut list) in self.map {
            if list.entries.is_empty() { continue }
            let gmr = gmr.to_owned();
            let proxy = proxy.clone();
            let thread = spawn(move ||
                if key == 0xb463cbdec08d6265 {
                    list.sync_aur(&gmr, &proxy, hold)
                } else {
                    list.sync_generic(&gmr, &proxy, hold)
                });
            threads.push(thread)
        }
        let mut r = Ok(());
        for thread in threads {
            match thread.join() {
                Ok(tr) => if let Err(e) = tr {
                    log::error!("A git sync thread has bad return");
                    r = Err(e)
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