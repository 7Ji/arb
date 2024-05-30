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
        #[derive(PartialEq)]
        struct ProvideChain<'a> {
            name: &'a str, // Main key
            version: &'a PlainVersion,
            package: &'a str,
            pkgbuild: &'a str,
            db_id: usize,
        }
        impl std::cmp::PartialOrd for ProvideChain<'_> {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                use std::cmp::Ordering;
                match self.name.partial_cmp(&other.name) {
                    Some(Ordering::Equal) => (),
                    ord => return ord,
                }
                match self.db_id.cmp(&other.db_id) {
                    Ordering::Equal => (),
                    ord => return Some(ord),
                }
                match self.package.partial_cmp(&other.package) {
                    Some(core::cmp::Ordering::Equal) => (),
                    ord => return ord,
                }
                match self.pkgbuild.partial_cmp(&other.pkgbuild) {
                    Some(core::cmp::Ordering::Equal) => (),
                    ord => return ord,
                }
                None
            }
        
        }
        let mut provide_chains = Vec::<ProvideChain>::new();
        let mut db_names = Vec::new();
        for (db_id, (db_name, db)) in 
            dbs.dbs.iter().enumerate() 
        {
            db_names.push(db_name);
            for package in db.packages.iter() {
                for provide in package.provides.iter() {
                    provide_chains.push(ProvideChain { 
                        name: &provide.name,
                        version: match &provide.version {
                            Some(version) => version,
                            None => &package.version,
                        },
                        package: &package.name,
                        pkgbuild: "", 
                        db_id,
                    })
                }
                provide_chains.push(ProvideChain { 
                    name: &package.name,
                    version: &package.version, 
                    package: &package.name, 
                    pkgbuild: "", 
                    db_id
                })
            }
        }
        #[derive(Debug)]
        struct PkgbuildDepends<'a> {
            pkgbuild: &'a Pkgbuild,
            deps: Vec<&'a Dependency>,
            build: BuildMethod,
        }
        impl std::fmt::Display for PkgbuildDepends<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "PKGBUILD {}: deps: ", self.pkgbuild.name)?;
                for dep in self.deps.iter() {
                    write!(f, "{}, ", dep)?;                    
                }
                write!(f, "build method: {:?}", self.build)?;
                Ok(())

            }
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
            pkgbuilds_depends.push(PkgbuildDepends {
                pkgbuild, deps, build: Default::default()})
        }
        let mut build_plan = BuildPlan::default();
        while ! pkgbuilds_depends.is_empty() {
            // Do this for every loop, as a sort is needed before first loop,
            // and provides from pkgbuilds could be added during last loop
            provide_chains.sort_unstable_by(|some, other|
                some.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal));
            // for provide_chain in provide_chains.iter() {
            //     log::info!("Provide {} <= package {} <= db {} / PKGBUILD {}", 
            //         provide_chain.name, provide_chain.package, db_names[provide_chain.db_id],
            //         provide_chain.pkgbuild);
            // }
            // log::info!("Provide chain first {}, last {}", 
            // provide_chains[0].name, provide_chains[provide_chains.len() - 1].name);
                // |provide_chain|provide_chain.name);
            let mut build_stage = BuildStage::default();
            // let mut build_stage = 
            for pkgbuild_depends in 
                pkgbuilds_depends.iter_mut() 
            {
                let mut i = 0;
                while i < pkgbuild_depends.deps.len() {
                    let dep = &pkgbuild_depends.deps[i];
                    match provide_chains.binary_search_by_key(
                        &dep.name.as_str(), 
                        |provide_chain|provide_chain.name
                    ) {
                        Ok(id) => {
                            let mut id_start = id;
                            while id_start > 0 && provide_chains[id_start - 1].name == dep.name {
                                id_start -= 1;
                            }
                            let mut id_end = id;
                            while provide_chains[id_end + 1].name == dep.name {
                                id_end += 1;
                            }
                            let mut candidates = Vec::new();
                            candidates.reserve(id_end - id_start + 1);
                            for j in id_start..id_end + 1 {
                                candidates.push((j, &provide_chains[j]))
                            }
                            if let Some(ordered_version) = &dep.version {
                                let plain_version = &ordered_version.plain;
                                candidates.retain(|(_, provide_chain)|{
                                    match ordered_version.order {
                                        pkgbuild::DependencyOrder::Greater => 
                                            provide_chain.version > plain_version,
                                        pkgbuild::DependencyOrder::GreaterOrEqual => 
                                            provide_chain.version >= plain_version,
                                        pkgbuild::DependencyOrder::Equal => 
                                            provide_chain.version == plain_version,
                                        pkgbuild::DependencyOrder::LessOrEqual => 
                                            provide_chain.version <= plain_version,
                                        pkgbuild::DependencyOrder::Less => 
                                            provide_chain.version < plain_version,
                                    }
                                });
                                if candidates.is_empty() {
                                    i += 1;
                                    continue
                                }
                            }
                            candidates.sort_unstable_by(|some, other|
                                some.1.partial_cmp(other.1).unwrap_or(std::cmp::Ordering::Equal));
                            let mut id_match = candidates[0].0;
                            // Prefer the one with exact name match, otherwise the first one
                            for candidate in candidates.iter() {
                                if candidate.1.package == dep.name {
                                    id_match = candidate.0;
                                    break;
                                }
                            }
                            let provide_chain = &provide_chains[id_match];
                            if provide_chain.db_id == usize::MAX {
                                &mut pkgbuild_depends.build.install_built
                            } else {
                                build_plan.cache.push(provide_chain.package.into());
                                &mut pkgbuild_depends.build.install_repo
                            }.push(provide_chain.package.into());
                            pkgbuild_depends.deps.swap_remove(i);
                        },
                        Err(_) => {
                            log::warn!("Did not find provider for {}", dep.name);
                            i += 1
                        },
                    };
                }
            }
            let len_last = pkgbuilds_depends.len();
            let mut i = 0;
            while i < pkgbuilds_depends.len() {
                let pkgbuild_depends = &pkgbuilds_depends[i];
                if pkgbuild_depends.deps.is_empty() {
                    let pkgbuild_depends = 
                        pkgbuilds_depends.swap_remove(i);
                    let mut build = pkgbuild_depends.build;
                    build.pkgbuild = pkgbuild_depends.pkgbuild.name.clone();
                    build.install_repo.sort_unstable();
                    build.install_repo.dedup();
                    build.install_built.sort_unstable();
                    build.install_built.dedup();
                    build_stage.build.push(build);
                    let pkgbuild_name = &pkgbuild_depends.pkgbuild.name;
                    macro_rules! add_provide_chains {
                        ($parent: expr, $package: expr) => {
                            for provide in $parent.provides.iter() {
                                provide_chains.push(ProvideChain { 
                                    name: &provide.name,
                                    version: match &provide.version {
                                        Some(version) => version,
                                        None => &pkgbuild_depends.pkgbuild.inner.version
                                    },
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
                    let pkgbuild = &pkgbuild_depends.pkgbuild.inner;
                    add_provide_chains_multiarch!(pkgbuild, pkgbuild.pkgbase);
                    for package in pkgbuild.pkgs.iter() {
                        add_provide_chains_multiarch!(package, package.pkgname);
                        provide_chains.push(ProvideChain { 
                            name: &package.pkgname,
                            version: &pkgbuild.version,
                            package: &package.pkgname,
                            pkgbuild: &pkgbuild_name, 
                            db_id: usize::MAX,
                        })
                    }
                    if pkgbuild.pkgs.is_empty() {
                        provide_chains.push(ProvideChain { 
                            name: &pkgbuild_name,
                            version: &pkgbuild.version,
                            package: &pkgbuild_name,
                            pkgbuild: &pkgbuild_name, 
                            db_id: usize::MAX,
                        })
                    }
                } else {
                    i += 1
                }
            }
            if pkgbuilds_depends.len() == len_last {
                log::error!("Failed to resolve dependency: {} PKGBUILDs not \
                    resolved:", len_last);
                for pkgbuild_depend in pkgbuilds_depends.iter() {
                    log::error!("PKGBUILD not resolved: {}", pkgbuild_depend)
                }
                return Err(Error::BrokenPKGBUILDs(
                    pkgbuilds_depends.iter().map(
                        |pkgbuild_depends|
                            pkgbuild_depends.pkgbuild.name.clone()).collect()))
            }
            build_plan.stages.push(build_stage);
        }
        // Now all is done
        log::info!("Build plan: cache packages: {:?}", build_plan.cache);
        for (id, stage) in build_plan.stages.iter().enumerate() {
            log::info!("Build stage {}:", id);
            for build in &stage.build {
                log::info!("- Build {}, install from repo: {:?}, install built: {:?}",
                    build.pkgbuild, build.install_repo, build.install_built);
            }
        }
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

#[derive(Debug, Default)]
struct BuildMethod {
    pkgbuild: String,
    install_repo: Vec<String>,
    install_built: Vec<String>,
}

#[derive(Default)]
struct BuildStage {
    /// Install these dependencies before build, these could come from either
    /// the 
    // install: Vec<String>,
    /// Build these PKGBUILDs (not packages) concurrently
    build: Vec<BuildMethod>,
}

#[derive(Default)]
pub(crate) struct BuildPlan {
    /// Cache these packages from Internet before any stage
    cache: Vec<String>,
    stages: Vec<BuildStage>,
}