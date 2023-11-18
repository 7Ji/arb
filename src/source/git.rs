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
        thread,
    };

use crate::{
        error::{
            Error,
            Result
        },
        source::{
            aur::AurResult,
            Proxy
        },
        threading
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
        -> Result<Repo>
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
                log::error!(
                    "Failed to open bare repo for git source '{}'",
                    url);
                Err(())
            },
        }
    }

    fn to_repos_map(
        map: HashMap<u64, Vec<Self>>, parent: &str, gmr: Option<&Gmr>
    ) -> Result<HashMap<u64, Vec<Repo>>>
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
                    log::error!("Duplicated key for repos map");
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
    proxy: Option<&Proxy>,
    refspecs: &[&str],
    tries: usize
) -> Result<()>
{
    let (tries_without_proxy, tries_with_proxy) = match proxy {
        Some(proxy) => (proxy.after, tries),
        None => (tries, 0),
    };
    for _ in 0..tries_without_proxy {
        match remote.fetch(
            refspecs, Some(fetch_opts), None
        ) {
            Ok(_) => return Ok(()),
            Err(e) => {
                log::error!("Failed to fetch from remote '{}': {}",
                    remote_safe_url(&remote), e);
            },
        }
    }
    let proxy = match proxy {
        Some(proxy) => proxy,
        None => {
            log::error!("Failed to fetch from remote '{}' after {} tries and \
                there's no proxy to retry, giving up", remote_safe_url(&remote),
                tries_without_proxy);
            return Err(())
        },
    };
    if tries_without_proxy > 0 {
        log::error!("Failed to fetch from remote '{}' after {} tries, will use \
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
            },
        }
    };
    log::error!("Failed to fetch from remote '{}' even with proxy",
        remote_safe_url(&remote));
    Err(())
}

impl Repo {
    fn add_remote(&self) -> Result<()> {
        match &self.repo.remote_with_fetch(
            "origin", &self.url, "+refs/*:refs/*") {
            Ok(_) => Ok(()),
            Err(e) => {
                log::error!("Failed to add remote {}: {}",
                            self.path.display(), e);
                std::fs::remove_dir_all(&self.path).or(Err(()))
            }
        }
    }

    fn init_bare<P: AsRef<Path>>(path: P, url: &str, gmr: Option<&Gmr>)
        -> Result<Self>
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
                log::error!("Failed to create {}: {}",
                            &path.as_ref().display(), e);
                Err(())
            }
        }
    }

    pub(crate) fn open_bare<P: AsRef<Path>>(
        path: P, url: &str, gmr: Option<&Gmr>
    ) -> Result<Self>
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
                    log::error!("Failed to open {}: {}",
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
        -> Result<()>
    {
        let url = remote_safe_url(remote);
        let heads = match remote.list() {
            Ok(heads) => heads,
            Err(e) => {
                log::error!("Failed to list remote '{}' for repo '{}': {}",
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
                            log::error!("Failed to set head for '{}': {}",
                                        url, e);
                        },
                    }
                }
                break
            }
        }
        Ok(())
    }

    fn _update_head(&self, remote: &mut Remote) -> Result<()> {
        Self::update_head_raw(&self.repo, remote)
    }

    fn sync_raw(
        repo: &Repository, url: &str, proxy: Option<&Proxy>, refspecs: &[&str],
        tries: usize, terminal: bool
    ) -> Result<()>
    {
        let mut remote =
            repo.remote_anonymous(url).or(Err(()))?;
        let mut fetch_opts = fetch_opts_init(terminal);
        fetch_remote(&mut remote, &mut fetch_opts, proxy, refspecs, tries)?;
        Self::update_head_raw(repo, &mut remote)?;
        Ok(())
    }

    pub(crate) fn sync(&self, proxy: Option<&Proxy>, terminal: bool)
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
                &self.repo, &mirror, None, refspecs, 1, terminal
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
                Err(())
            }
        }
    }

    fn get_branch_commit<'a>(&'a self, branch: &str) -> Result<Commit<'a>> {
        let branch_gref = self.get_branch(branch)?;
        match branch_gref.get().peel_to_commit() {
            Ok(commit) => Ok(commit),
            Err(e) => {
                log::error!("Failed to peel branch '{}' to commit: {}", branch, e);
                return Err(())
            },
        }
    }

    pub(crate) fn _get_branch_commit_id(&self, branch: &str) -> Result<Oid> {
        Ok(self.get_branch_commit(branch)?.id())
    }

    fn get_commit_tree<'a>(&'a self, commit: &Commit<'a>, subtree: Option<&Path>
    )   -> Result<Tree<'a>>
    {
        let tree = commit.tree().or_else(|e| {
            log::error!("Failed to get tree pointed by commit: {}", e);
            Err(())
        })?;
        let subtree = match subtree {
            Some(subtree) => subtree,
            None => return Ok(tree),
        };
        let entry = match tree.get_path(subtree) {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("Failed to get sub tree: {}", e);
                return Err(())
            },
        };
        Ok(entry.to_object(&self.repo).or_else(|e|{
            log::error!("Failed to convert entry to object: {}", e);
            Err(())
        })?
        .as_tree()
        .ok_or_else(||{
            log::error!("Failed to convert object ot ree")})?
        .to_owned())
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
        let tree = self.get_commit_tree(&commit, subtree)?;
        Ok(tree.id())
    }

    fn get_tree_entry_blob<'a>(&'a self, tree: &Tree, name: &str)
        -> Result<Blob<'a>>
    {
        let entry =
            match tree.get_name(name) {
                Some(entry) => entry,
                None => {
                    log::error!("Failed to find entry of {}", name);
                    return Err(())
                },
            };
        let object =
            match entry.to_object(&self.repo) {
                Ok(object) => object,
                Err(e) => {
                    log::error!("Failed to convert tree entry to object: {}", e);
                    return Err(())
                },
            };
        match object.into_blob() {
            Ok(blob) => Ok(blob),
            Err(_) => {
                log::error!("Failed to convert into a blob");
                return Err(())
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
        let job = format!("syncing git repos from domain '{}'",
                repos.last().ok_or(())?.get_domain());
        let mut bad = false;
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

            if let Err(_) =
                threading::wait_if_too_busy(&mut threads, max_threads, &job) {
                bad = true;
            }
            threads.push(thread::spawn(move ||{
                repo.sync(proxy_thread.as_ref(), terminal)
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

    fn sync_for_domain_st(
        repos: Vec<Self>,
        hold: bool,
        proxy: Option<&Proxy>,
        terminal: bool
    ) -> Result<()>
    {
        let mut bad = false;
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
            if repo.sync(proxy, terminal).is_err() {
                log::error!("Failed to sync repo '{}'", &repo.url);
                bad = true
            }
        }
        if bad {
            Err(())
        } else {
            Ok(())
        }
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

    fn filter_aur(repos: &mut Vec<Self>) -> Result<()> {
        let mut pkgs: Vec<String> = Vec::new();
        for repo in repos.iter() {
            let url = match Url::parse(&repo.url) {
                Ok(url) => url,
                Err(e) => {
                    log::error!("Failed to parse AUR url '{}': {}", &repo.url, e);
                    return Err(());
                },
            };
            let pkg = url.path()
                .trim_start_matches('/').trim_end_matches(".git");
            pkgs.push(pkg.to_string());
        }
        if pkgs.len() != repos.len() {
            log::error!("Pkgs and repos len mismatch");
            return Err(())
        }
        let mut aur_result = match AurResult::from_pkgs(&pkgs) {
            Ok(aur_result) => aur_result,
            Err(_) => {
                log::error!("Failed to get result from AUR RPC");
                return Err(())
            },
        };
        if aur_result.results.len() == repos.len() {
            let mut i = 0;
            while i < repos.len() {
                let repo = match repos.get(i) {
                    Some(repo) => repo,
                    None => {
                        log::error!("Failed to get repo");
                        return Err(())
                    },
                };
                let pkg = match aur_result.results.get(i) {
                    Some(pkg) => pkg,
                    None => {
                        log::error!("Failed to get pkg");
                        return Err(())
                    },
                };
                // leave a 1-min window
                if repo.last_fetch() > pkg.last_modified + 60 {
                    log::info!(
                        "Repo '{}' last fetch later than AUR last modified, \
                        skippping it", &repo.url);
                    repos.swap_remove(i);
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
            while i < repos.len() {
                let repo = match repos.get(i) {
                    Some(repo) => repo,
                    None => {
                        log::error!("Failed to get repo");
                        return Err(())
                    },
                };
                let pkg = match pkgs.get(i) {
                    Some(pkg) => pkg,
                    None => {
                        log::error!("Failed to get pkg");
                        return Err(())
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
                            return Err(())
                        },
                    };
                    if repo.last_fetch() > result.last_modified + 60 {
                        log::info!("Repo '{}' last fetch later than AUR last \
                            modified, skippping it", &repo.url);
                        repos.swap_remove(i);
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

    pub(crate) fn sync_mt(
        repos_map: HashMap<u64, Vec<Self>>,
        hold: bool,
        proxy: Option<&Proxy>,
        terminal: bool
    ) -> Result<()>
    {
        log::info!("Syncing repos with {} groups", repos_map.len());
        let mut threads = vec![];
        for (domain, repos) in repos_map {
            let proxy_thread = proxy.and_then(
                |proxy_actual|Some(proxy_actual.to_owned()));
            if domain == 0xb463cbdec08d6265 {
                threads.push(thread::spawn(move || {
                    Self::sync_for_aur(
                        repos, hold, proxy_thread.as_ref(), terminal)}))

            } else {
                threads.push(thread::spawn(move || {
                    Self::sync_for_domain(
                        repos, 10, hold,
                        proxy_thread.as_ref(), terminal)}))
            }
        }
        threading::wait_remaining(threads, "syncing git repo groups")
    }
}