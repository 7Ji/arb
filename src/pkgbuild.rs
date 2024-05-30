mod source;

use std::{collections::{BTreeMap, HashMap}, ffi::OsString, io::{stdout, Read, Write}, iter::{empty, once}, path::Path};
use git2::Oid;
use pkgbuild::{self, Architecture, Dependency, PlainVersion, Provide};
use crate::{config::{PersistentPkgbuildConfig, PersistentPkgbuildsConfig}, constant::PATH_PKGBUILDS, filesystem::{create_dir_allow_existing, set_current_dir_checked}, git::{RepoToOpen, ReposListToOpen}, mount::mount_bind, pacman::PacmanDbs, pkgbuild::source::CacheableSources, proxy::Proxy, rootless::{chroot_checked, set_uid_gid, BrokerPayload}, Error, Result};

#[derive(Debug)]
pub(crate) struct Pkgbuild {
    inner: pkgbuild::Pkgbuild,
    /// This is the name defined in config, not necessarily the same as 
    /// `inner.base`
    name: String, 
    url: String,
    branch: String,
    subtree: String,
    deps: Vec<String>,
    makedeps: Vec<String>,
    homebinds: Vec<String>,
    commit: Oid,
}

impl Pkgbuild {
    fn from_config(name: String, config: PersistentPkgbuildConfig) -> Self 
    {
        let (mut url, mut branch, mut subtree, 
            deps, makedeps, homebinds) = 
        match config {
            PersistentPkgbuildConfig::Simple(url) => (
                url, Default::default(), Default::default(), Default::default(), 
                Default::default(), Default::default()),
            PersistentPkgbuildConfig::Complex { url, branch, 
                subtree, deps, makedeps, 
                homebinds } => (
                    url, branch, subtree, deps, makedeps, homebinds),
        };
        if url  == "AUR" {
            url = format!("https://aur.archlinux.org/{}.git", name)
        } else if url.starts_with("GITHUB/") {
            if url.ends_with('/') {
                url = format!(
                    "https://github.com/{}{}.git", &url[7..], name)
            } else {
                url = format!(
                    "https://github.com/{}.git", &url[7..])
            }
        } else if url.starts_with("GH/") {
            if url.ends_with('/') {
                url = format!(
                    "https://github.com/{}{}.git", &url[3..], name)
            } else {
                url = format!(
                    "https://github.com/{}.git", &url[3..])
            }
        }
        if branch.is_empty() {
            branch = "master".into()
        }
        if subtree.ends_with('/') {
            subtree.push_str(&name)
        }
        subtree = subtree.trim_start_matches('/').into();
        Self {
            inner: Default::default(),
            name,
            url,
            branch,
            subtree,
            deps,
            makedeps,
            homebinds,
            commit: git2::Oid::zero(),
        }
    }

    fn dump<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        log::debug!("Dumping PKGBUILD '{}'", &self.name);
        RepoToOpen::new_with_url_parent(&self.url, "PKGBUILD")
            .try_open_only()?
            .dump_branch_pkgbuild(
                &self.branch, self.subtree.as_ref(), path.as_ref())
    }
}

#[derive(Default, Debug)]
pub(crate) struct Pkgbuilds {
    pub(crate) pkgbuilds: Vec<Pkgbuild>,
}

impl From<PersistentPkgbuildsConfig> for Pkgbuilds {
    fn from(config: PersistentPkgbuildsConfig) -> Self {
        let mut pkgbuilds: Vec<Pkgbuild> = config.into_iter().map(
            |(name, config)| {
                Pkgbuild::from_config(name, config)
            }).collect();
        pkgbuilds.sort_unstable_by(
            |a,b|a.name.cmp(&b.name));
        Self {
            pkgbuilds,
        }
    }
}

impl Into<Vec<Pkgbuild>> for Pkgbuilds {
    fn into(self) -> Vec<Pkgbuild> {
        self.pkgbuilds
    }
}

impl Pkgbuilds {
    pub(crate) fn git_urls(&self) -> Vec<String> {
        self.pkgbuilds.iter().map(
            |repo|repo.url.clone()).collect()
    }

    pub(crate) fn complete_from_reader<R: Read>(&mut self, reader: R) -> Result<()> {
        let pkgbuilds_raw: Vec<pkgbuild::Pkgbuild> = match rmp_serde::from_read(reader) {
            Ok(pkgbuilds_raw) => pkgbuilds_raw,
            Err(e) => {
                log::error!("Failed to read raw PKGBUILDs from reader: {}", e);
                return Err(e.into())
            },
        };
        let count_parsed = pkgbuilds_raw.len();
        let count_config = self.pkgbuilds.len();
        if count_parsed != count_config {
            log::error!("Parsed PKGBUILDs count different from input: \
                parsed {}, config {}", count_parsed, count_config);
            return Err(Error::ImpossibleLogic)
        }
        for (pkgbuild_wrapper, pkgbuild_raw) in 
            self.pkgbuilds.iter_mut().zip(pkgbuilds_raw.into_iter()) 
        {
            pkgbuild_wrapper.inner = pkgbuild_raw;
        }
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pkgbuilds.is_empty()
    }

    /// Sync the PKGBUILDs repos from remote
    pub(crate) fn sync(&self, gmr: &str, proxy: &Proxy, hold: bool) 
        -> Result<()> 
    {
        log::info!("Syncing PKGBUILDs (gmr: '{}', proxy: {}, hold: {})...",
            gmr, proxy, hold);
        let mut repos_list = ReposListToOpen::default();
        for pkgbuild in self.pkgbuilds.iter() {
            repos_list.add::<_, _, _, _, _, &str>(
                "PKGBUILD", &pkgbuild.url, 
                once(&pkgbuild.branch), empty())
        }
        repos_list.try_open_init_into_map()?.sync(gmr, proxy, hold)?;
        log::info!("Synced PKGBUILDs");
        Ok(())
    }

    pub(crate) fn dump<P: AsRef<Path>>(&self, parent: P) -> Result<()> {
        log::info!("Dumping PKGBUILDs...");
        let parent = parent.as_ref();
        create_dir_allow_existing(&parent)?;
        for pkgbuild in self.pkgbuilds.iter() {
            let path_pkgbuild = parent.join(&pkgbuild.name);
            pkgbuild.dump(&path_pkgbuild)?
        }
        log::info!("Dumped PKGBUILDs");
        Ok(())
    }

    pub(crate) fn get_reader_payload(&self, root: &Path) -> BrokerPayload {
        let mut payload = BrokerPayload::new_with_root(root);
        let root: OsString = root.into();
        payload.add_init_command_run_applet(
            "read-pkgbuilds",
            once(root).chain(
            self.pkgbuilds.iter().map(
                |pkgbuild|(&pkgbuild.name).into())));
        payload
    }

    pub(crate) fn get_cacheable_sources(&self, arch: Option<&Architecture>) 
        -> CacheableSources 
    {
        let sources = 
            CacheableSources::from_pkgbuilds(self, arch);
        log::debug!("Cacheable sources: {:?}", &sources);
        sources
    }

    pub(crate) fn get_plans(&self, dbs: &PacmanDbs, arch: &Architecture) 
        -> Result<BuildPlan> 
    {
        struct ProvideChain<'a> {
            name: &'a str, // Main key
            version: Option<&'a PlainVersion>,
            package: &'a str,
            pkgbuild: &'a str,
            db_id: usize,
        }
        let mut provide_chains = Vec::<ProvideChain>::new();
        // for pkgbuild in self.pkgbuilds.iter() {
        //     let pkgbuild_name = &pkgbuild.name;
        //     let pkgbuild = &pkgbuild.inner;
        //     macro_rules! add_provide_chains {
        //         ($parent: expr, $package: expr) => {
        //             for provide in $parent.provides.iter() {
        //                 provide_chains.push(ProvideChain { 
        //                     name: &provide.name,
        //                     version: provide.version.as_ref(),
        //                     package: &$package,
        //                     pkgbuild: &pkgbuild_name, 
        //                     db_id: usize::MAX,
        //                 })
        //             }
        //         };
        //     }
        //     macro_rules! add_provide_chains_multiarch {
        //         ($parent: expr, $package: expr) => {
        //             add_provide_chains!($parent.multiarch.any, $package);
        //             if let Some(arch_specific) = 
        //                 $parent.multiarch.arches.get(arch) 
        //             {
        //                 add_provide_chains!(arch_specific, $package);
        //             }
        //         };
        //     }
        //     add_provide_chains_multiarch!(pkgbuild, pkgbuild.pkgbase);
        //     for package in pkgbuild.pkgs.iter() {
        //         add_provide_chains_multiarch!(package, package.pkgname);
        //     }
        // }
        let mut db_names = Vec::new();
        for (db_id, (db_name, db)) in 
            dbs.dbs.iter().enumerate() 
        {
            db_names.push(db_name);
            for package in db.packages.iter() {
                for provide in package.provides.iter() {
                    provide_chains.push(ProvideChain { 
                        name: &provide.name,
                        version: provide.version.as_ref(),
                        package: &package.name,
                        pkgbuild: "", 
                        db_id,
                    })
                }
            }
        }
        provide_chains.sort_unstable_by_key(
            |provide_chain|provide_chain.name);
        #[derive(Debug)]
        struct PkgbuildDepends<'a> {
            pkgbuild: &'a Pkgbuild,
            deps: Vec<&'a Dependency>,
        }
        let mut pkgbuilds_depends = 
            Vec::<PkgbuildDepends>::new();
        for pkgbuild in self.pkgbuilds.iter() {
            let mut deps = Vec::new();
            macro_rules! add_deps {
                ($arch_specific: ident, $($depends: ident),+) => {
                    $(
                        for dep in $arch_specific.$depends.iter()
                        {
                            deps.push(dep)
                        }
                    )+
                };
            }
            macro_rules! add_deps_multiarch {
                ($parent: expr, $($depends: ident),+) => {
                    let arch_specific = &$parent.multiarch.any;
                    add_deps!(arch_specific, $($depends),+);
                    if let Some(arch_specific) = 
                        pkgbuild.inner.multiarch.arches.get(arch) 
                    {
                        add_deps!(arch_specific, $($depends),+);
                    }
                };
            }
            add_deps_multiarch!(
                pkgbuild.inner, depends, makedepends,checkdepends);
            for package in pkgbuild.inner.pkgs.iter() {
                add_deps_multiarch!(package, depends, checkdepends);
            }
            pkgbuilds_depends.push(PkgbuildDepends {pkgbuild, deps})
        }
        // pkgbuilds_depends.sort_unstable_by_key(
        //     |pkgbuild_depends|pkgbuild_depends.name);
        let mut build_plan = BuildPlan::default();
        while ! pkgbuilds_depends.is_empty() {
            let mut build_stage = BuildStage::default();
            // let mut build_stage = 
            for pkgbuild_depends in 
                pkgbuilds_depends.iter_mut() 
            {

                for dep in pkgbuild_depends.deps.iter() {
                    // provide_chains.parti
                    for provide_chain in provide_chains.iter() {
                    }
                }
            }
            let len_last = pkgbuilds_depends.len();
            pkgbuilds_depends.retain(|pkgbuild_depends|
                if pkgbuild_depends.deps.is_empty() {
                    let pkgbuild = pkgbuild_depends.pkgbuild;
                    let pkgbuild_name = &pkgbuild.name;
                    build_stage.build.push(pkgbuild_name.into());
                    let pkgbuild = &pkgbuild.inner;
                    macro_rules! add_provide_chains {
                        ($parent: expr, $package: expr) => {
                            for provide in $parent.provides.iter() {
                                provide_chains.push(ProvideChain { 
                                    name: &provide.name,
                                    version: provide.version.as_ref(),
                                    package: &$package,
                                    pkgbuild: &pkgbuild_name, 
                                    db_id: usize::MAX,
                                })
                            }
                        };
                    }
                    macro_rules! add_provide_chains_multiarch {
                        ($parent: expr, $package: expr) => {
                            add_provide_chains!($parent.multiarch.any, $package);
                            if let Some(arch_specific) = 
                                $parent.multiarch.arches.get(arch) 
                            {
                                add_provide_chains!(arch_specific, $package);
                            }
                        };
                    }
                    add_provide_chains_multiarch!(pkgbuild, pkgbuild.pkgbase);
                    for package in pkgbuild.pkgs.iter() {
                        add_provide_chains_multiarch!(package, package.pkgname);
                    }
                    false
                } else {
                    true
                }
            );
            if pkgbuilds_depends.len() == len_last {
                log::error!("Failed to resolve dependency: {} PKGBUILDs not \
                    resolved: {:?}", len_last, pkgbuilds_depends);
                return Err(Error::BrokenPKGBUILDs(
                    pkgbuilds_depends.iter().map(
                        |pkgbuild_depends|
                            pkgbuild_depends.pkgbuild.name.clone()).collect()))
            }
        }
        // Now let's look up
        Ok(build_plan)
    }
}

/// The `pkgbuild_reader` applet entry point, takes no args
pub(crate) fn action_read_pkgbuilds<P1, I, P2>(root: P1, pkgbuilds: I) 
    -> Result<()> 
where
    P1: AsRef<Path>,
    I: IntoIterator<Item = P2>,
    P2: AsRef<Path>
{
    // Out parent (init) cannot chroot, as that would result in parent init 
    // possibly failing to call us, due to libc, libgit, etc being in different 
    // versions
    log::info!("Reading PKGBUILDs...");
    let path_root = root.as_ref();
    let path_pkgbuilds = path_root.join("PKGBUILDs");
    create_dir_allow_existing(&path_pkgbuilds)?;
    mount_bind(PATH_PKGBUILDS, &path_pkgbuilds)?;
    set_current_dir_checked(&path_pkgbuilds)?;
    chroot_checked("..")?;
    set_uid_gid(1000, 1000)?;
    let pkgbuilds = match pkgbuild::parse_multi(pkgbuilds)
    {
        Ok(pkgbuilds) => pkgbuilds,
        Err(e) => {
            log::error!("Failed to parse PKGBUILDs: {}", e);
            return Err(e.into())
        },
    };
    log::info!("Parsed {} PKGBUILDs, writing to stdout to pass them back to \
        parent...", pkgbuilds.len());
    let output = match rmp_serde::to_vec(&pkgbuilds) {
        Ok(output) => output,
        Err(e) => {
            log::error!("Failed to encode output: {}", e);
            return Err(e.into())
        },
    };
    if let Err(e) = stdout().write_all(&output) {
        log::error!("Failed to write serialized output to stdout: {}", e);
        Err(e.into())
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct BuildStage {
    /// Install these dependencies before build, these could come from either
    /// the 
    install: Vec<String>,
    /// Build these PKGBUILDs (not packages) concurrently
    build: Vec<String>,
}

#[derive(Default)]
pub(crate) struct BuildPlan {
    /// Cache these packages from Internet before any stage
    cache: Vec<String>,
    stages: Vec<BuildStage>,
}