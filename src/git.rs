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
use nix::NixPath;
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};
use url::Url;
use xxhash_rust::xxh3::xxh3_64;
// use threadpool::ThreadPool;
// use url::Url;
use std::{
        collections::HashMap, fmt::Display, fs::{metadata, File}, io::Write, os::unix::fs::MetadataExt, path::{
            Path,
            PathBuf
        }, str::FromStr
    };

use crate::{aur::AurResult, proxy::{Proxy, NOPROXY}, Error, Result};

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

pub(crate) struct RepoToOpen {
    pub(crate) path: PathBuf,
    pub(crate) url: String,
}

impl RepoToOpen {
    pub(crate) fn new_with_url_parent<S, P>(url: S, parent: P) -> Self 
    where
        S: Into<String>,
        P: AsRef<Path>
    {
        let url: String = url.into();
        let xxh3_63 = xxh3_64(url.as_bytes());
        let path = parent.as_ref().join(
            format!("{:016x}", xxh3_63));
        Self { path, url }
    }

    pub(crate) fn new_with_url_parent_type<S, D>(url: S, parent_type: D) 
        -> Self 
    where
        S: Into<String>,
        D: Display
    {
        let url: String = url.into();
        let xxh3_63 = xxh3_64(url.as_bytes());
        let path = PathBuf::from(
            format!("sources/{}/{:016x}", parent_type ,xxh3_63));
        Self { path, url }
    }
}

impl TryInto<Repo> for RepoToOpen {
    type Error = Error;

    fn try_into(self) -> Result<Repo> {
        Repo::try_new_with_path_url(self.path, &self.url)
    }
}

pub(crate) struct Repo {
    pub(crate) path: PathBuf,
    pub(crate) url: String,
    pub(crate) repo: Repository,
    pub(crate) refspecs: Vec<String>,
}

fn refspec_same_branch_or_all(branch: &str) -> String {
    if branch.is_empty() {
        "+refs/heads/*:refs/heads/*".into()
    } else {
        format!("+refs/heads/{}:refs/heads/{}", branch, branch)
    }
}

type ReposVec = Vec<Repo>;

#[derive(Default)]
pub(crate) struct ReposList {
    pub(crate) list: ReposVec
}

impl From<Vec<Repo>> for ReposList {
    fn from(list: Vec<Repo>) -> Self {
        Self {list}
    }
}

impl Into<Vec<Repo>> for ReposList {
    fn into(self) -> Vec<Repo> {
        self.list
    }
}

impl ReposList {
    fn add_repo(&mut self, repo: Repo) {
        self.list.push(repo)
    }

    pub(crate) fn _from_iter<I, R>(iter: I) -> Result<Self> 
    where
        I: IntoIterator<Item = R>,
        R: TryInto<Repo, Error = Error>
    {
        let mut list = Self::default();
        for item in iter.into_iter() {
            list.add_repo(item.try_into()?)
        }
        Ok(list)
    }
}

pub(crate) type ReposHashMap = HashMap<u64, ReposList>;

#[derive(Default)]
pub(crate) struct ReposMap {
    pub(crate) map: ReposHashMap
}

impl ReposMap {
    fn try_add_repo(&mut self, repo: Repo) -> Result<()> {
        let key = match Url::parse(&repo.url) {
            Ok(url) => if let Some(domain) = url.domain() {
                xxh3_64(domain.as_bytes())
            } else {
                log::warn!("Repo URL '{}' does not have domain", url);
                0
            },
            Err(e) => {
                log::warn!("Failed to parse repo URL '{}': {}", repo.url, e);
                0
            },
        };
        match self.map.get_mut(&key) {
            Some(list) => list.add_repo(repo),
            None => if self.map.insert(key, vec![repo].into()).is_some()
            {
                log::error!("Impossible key {} already in map", key);
                return Err(Error::ImpossibleLogic)
            },
        }
        Ok(())
    }

    pub(crate) fn from_iter_into_repo_to_open<I, R>(iter: I) -> Result<Self>
    where
        I: IntoIterator<Item = R>,
        R: Into<RepoToOpen>
    {
        let mut map_to_open: HashMap<u64, Vec<RepoToOpen>> = HashMap::new();
        for item in iter.into_iter() {
            let repo_to_open: RepoToOpen = item.into();
            let key = match Url::parse(&repo_to_open.url) {
                Ok(url) => if let Some(domain) = url.domain() {
                    xxh3_64(domain.as_bytes())
                } else {
                    log::warn!("Repo URL '{}' does not have domain", url);
                    0
                },
                Err(e) => {
                    log::warn!("Failed to parse repo URL '{}': {}", 
                        repo_to_open.url, e);
                    0
                },
            };
            match map_to_open.get_mut(&key) {
                Some(list) => list.push(repo_to_open),
                None => 
                if map_to_open.insert(key, vec![repo_to_open]).is_some() {
                    log::error!("Impossible key {} already in repos map", key);
                    return Err(Error::ImpossibleLogic)
                },
            }
        }
        let mut map = HashMap::new();
        for (key, mut list_to_open) in map_to_open {
            list_to_open.sort_unstable_by(
                |some, other|
                    some.path.cmp(&other.path));
            list_to_open.dedup_by(
                |some, other|
                    some.path == other.path);
            if list_to_open.is_empty() { continue }
            let mut list = Vec::new();
            for repo_to_open in list_to_open {
                list.push(repo_to_open.try_into()?)
            }
            if map.insert(key, ReposList { list } ).is_some() {
                log::error!("Impossible key {} already in repos map", key);
                return Err(Error::ImpossibleLogic)
            }
        }
        Ok(Self { map })
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
    if tries_with_proxy == 0 {
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
    proxy_opts.url(proxy.get_url());
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
    pub(crate) fn try_new_with_path_url<P: Into<PathBuf>, S: Into<String>>(
        path: P, url: S) -> Result<Self>
    {
        let path = path.into();
        let url = url.into();
        match Repository::open_bare(&path) {
            Ok(repo) => Ok(Self {
                path,
                url,
                repo,
                refspecs: Default::default(),
            }),
            Err(e) => {
                if e.class() == ErrorClass::Os &&
                e.code() == ErrorCode::NotFound {
                    match Repository::init_bare(&path) {
                        Ok(repo) => {
                            let repo = Self {
                                path,
                                url,
                                repo,
                                refspecs: Default::default(),
                            };
                            repo.add_remote()?;
                            Ok(repo)
                        },
                        Err(e) => {
                            log::error!("Failed to create {}: {}",
                                        path.display(), e);
                            Err(e.into())
                        }
                    }
                } else {
                    log::error!("Failed to open {}: {}", path.display(), e);
                    Err(e.into())
                }
            },
        }
    }

    pub(crate) fn try_new_with_url_branch(
        url: &str, subtype: &str, branch: &str
    ) -> Result<Self> 
    {
        let path = format!("sources/{}/{:016x}", 
            subtype, xxh3_64(url.as_bytes()));
        let mut repo = Self::try_new_with_path_url(&path, url)?;
        repo.refspecs.push(refspec_same_branch_or_all(branch));
        Ok(repo)
    }

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
        Self::sync_raw(&self.repo, &self.url, proxy, refspecs, 3)?;
        log::info!("Synced repo '{}' with '{}' ",
                        &self.path.display(), &self.url);
        Ok(())
    }

    fn sync(&self, gmr: &str, proxy: &Proxy, hold: bool) -> Result<()>
    {
        if hold && self.is_head_healthy() {
            Ok(())
        } else if self.refspecs.is_empty() {
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
    fn get_commit_tree<'a>(&'a self, commit: &Commit<'a>, subtree: &Path
    )   -> Result<Tree<'a>>
    {
        let tree = match commit.tree() {
            Ok(tree) => tree,
            Err(e) => {
                log::error!("Failed to get tree pointed by commit: {}", e);
                return Err(e.into())
            },
        };
        if subtree.is_empty() {
            return Ok(tree)
        }
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

    fn get_branch_tree<'a>(&'a self, branch: &str, subtree: &Path)
        -> Result<Tree<'a>>
    {
        let commit = self.get_branch_commit(branch)?;
        self.get_commit_tree(&commit, subtree)
    }

    pub(crate) fn _get_branch_commit_or_subtree_id(&self,
        branch: &str, subtree: &Path
    ) -> Result<Oid>
    {
        let commit = self.get_branch_commit(branch)?;
        if subtree.is_empty() {
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
        branch: &str, subtree: &Path, name: &str
    )
        -> Result<Blob<'a>>
    {
        let tree = self.get_branch_tree(branch, subtree)?;
        self.get_tree_entry_blob(&tree, name)
    }

    /// A shortcut to `get_branch_entry_blob(branch, subtree, "PKGBUILD")`
    pub(crate) fn get_branch_pkgbuild(&self, branch: &str, subtree: &Path
    ) -> Result<Blob>
    {
        self.get_branch_entry_blob(branch, subtree, "PKGBUILD")
    }

    pub(crate) fn dump_branch_pkgbuild(
        &self, branch: &str, subtree: &Path, out: &Path
    ) -> Result<()>
    {
        let blob = self.get_branch_pkgbuild(branch, subtree)?;
        File::create(out)?.write_all(blob.content())?;
        Ok(())
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
    pub(crate) fn _checkout<P>(&self, target: P, branch: &str, subtree: &Path
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
    fn _get_domain_safe(&self) -> String {
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
                    consider I64_MIN ({}): {}", i64::MIN, e);
                return i64::MIN
            },
        };
        metadata.mtime()
    }
}

impl ReposList {
    fn keep_aur_outdated(&mut self) -> Result<()> {
        let mut pkgs: Vec<String> = Vec::new();
        for repo in self.list.iter() {
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
        if pkgs.len() != self.list.len() {
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
        if aur_result.len() != self.list.len() {
            log::error!("AUR results length mismatch repos len");
            return Err(Error::ImpossibleLogic)
        }
        for index in (0..self.list.len()).rev() {
            let mtime_repo = self.list[index].get_last_fetch();
            let mtime_pkg = aur_result.results[index].last_modified;
            let (mtime_pkg_delayed, overflowed) = 
                mtime_pkg.overflowing_add(60);
            if !overflowed && mtime_repo >= mtime_pkg_delayed{ 
                let repo = self.list.swap_remove(index);
                log::debug!("Repo '{}' last fetch ({}) later than AUR last \
                    modified ({}), skippping it", &repo.url, 
                    mtime_repo, mtime_pkg_delayed);
            }
        }
        log::info!("Filtered AUR repos");
        Ok(())
    }

    fn keep_unhealthy(&mut self) {
        self.list.retain(|repo|!repo.is_head_healthy())
    }

    fn sync_single_thread(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        self.list.iter_mut().try_for_each(|repo|
            repo.sync(gmr, proxy, hold))
    }

    fn sync_multi_threaded(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        self.list.par_iter_mut().try_for_each(|repo|
            repo.sync(gmr, proxy, hold))
    }

    fn sync_generic(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        self.sync_multi_threaded(gmr, proxy, hold)
    }

    fn sync_aur(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        self.keep_aur_outdated()?;
        // Length would change after filtering
        if self.list.is_empty() { return Ok(()) }
        log::info!("Syncing {} AUR repos, note that AUR repos can only be \
            synced in single thread, to avoid DoSing the AUR server",
            self.list.len());
        self.sync_single_thread(gmr, proxy, hold)
    }
}

impl ReposMap {
    fn keep_unhealthy(&mut self) {
        self.map.iter_mut().for_each(|(_, list)|
            list.keep_unhealthy());
        self.map.retain(|_, list|!list.list.is_empty())
    }

    pub(crate) fn sync(&mut self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        if hold {
            self.keep_unhealthy()
        }
        if self.map.is_empty() {
            return Ok(())
        }
        self.map.par_iter_mut().try_for_each(
            |(domain, list)| 
        {
            if *domain == 0xb463cbdec08d6265 { // AUR
                list.sync_aur(gmr, proxy, hold)
            } else {
                list.sync_generic(gmr, proxy, hold)
            }
        })
    }
}