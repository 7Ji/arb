use std::{iter::once, path::Path};

use pkgbuild::{Architecture, PlainVersion};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

// Worker is a finite state machine
use crate::{cli::ActionArgs, config::{PersistentConfig, RuntimeConfig}, constant::{PATH_BUILD, PATH_PKGBUILDS, PATH_ROOT_SINGLE_BASE}, git::gmr_config_from_urls, io::write_all_to_file_or_stdout, pacman::PacmanDbs, pkgbuild::BuildPlan, rootless::{BaseRoot, Root, RootlessHandler}, Error, Result};

pub(crate) struct WorkerStateReadConfig {
    config: PersistentConfig
}

impl WorkerStateReadConfig {
    /// Create a `WorkerState` by reading config, this is the only way a 
    /// `WorkerState` could be legally created
    pub(crate) fn try_new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self { config: PersistentConfig::try_read(path)? })
    }

    /// Merge arguments with persistent config (config file) to get runtime
    /// config
    pub(crate) fn try_merge_config(self, args: ActionArgs) 
        -> Result<WorkerStateMergedConfig> 
    {
        let config = 
            RuntimeConfig::try_from((args, self.config))?;
        if config.pkgbuilds.is_empty() { 
            log::error!("No PKGBUILDs defined");
            return Err(Error::InvalidConfig)
        }
        if ! config.gengmr.is_empty() {
            let gmr_config = gmr_config_from_urls(
                &mut config.pkgbuilds.git_urls());
            log::debug!("Generated git-mirroer config: {}", &gmr_config);
            write_all_to_file_or_stdout(
                &gmr_config, &config.gengmr)?
        }
        Ok(WorkerStateMergedConfig { config })

    }
}

pub(crate) struct WorkerStateMergedConfig {
    config: RuntimeConfig
}

impl WorkerStateMergedConfig {
    /// Prepare rootless handler so we could utilize user namespaces later
    pub(crate) fn try_prepare_rootless(self) 
        -> Result<WorkerStatePreparedRootless> 
    {
        Ok(WorkerStatePreparedRootless { 
            config: self.config, 
            rootless: RootlessHandler::try_new()?})
    }
}

pub(crate) struct WorkerStatePreparedRootless {
    config: RuntimeConfig,
    rootless: RootlessHandler
}

impl WorkerStatePreparedRootless {
    /// Prepare the work directory layout, by removing some stuffs first then
    /// creating some stuffs. Note this would utilize user namespaces to remove
    /// the `build` folder, to avoid failing due to different user IDs (non-root
    /// in child namespace, non-self in parent namespace). But that would still
    /// fail when the current user changed their ID mapping and lost root 
    /// identity in the original user namespace.
    pub(crate) fn try_prepare_layout(mut self) 
        -> Result<WorkerStatePreparedLayout>
    {
        self.rootless.run_action_no_payload(
            "rm-rf", once(PATH_BUILD))?;
        crate::filesystem::prepare_layout()?;
        self.config.paconf.set_defaults();
        Ok(WorkerStatePreparedLayout { 
            config: self.config, 
            rootless: self.rootless })
    }
}

pub(crate) struct WorkerStatePreparedLayout {
    config: RuntimeConfig,
    rootless: RootlessHandler,
}

impl WorkerStatePreparedLayout {
    /// Fetching PKGBUILD repos, deduping first. Different PKGBUILDs from a 
    /// single repo would only be fetched once. All PKGBUILDs would be stored
    /// locally in bare git repos to save disk space.
    pub(crate) fn try_fetch_pkgbuilds(self) 
        -> Result<WorkerStateFetchedPkgbuilds> 
    {
        let config = &self.config;
        config.pkgbuilds.sync(
            &config.gmr, &config.proxy, config.holdpkg)?;
        Ok(WorkerStateFetchedPkgbuilds { 
            config: self.config, 
            rootless: self.rootless 
        })
    }
}

pub(crate) struct WorkerStateFetchedPkgbuilds {
    config: RuntimeConfig,
    rootless: RootlessHandler,
}

impl WorkerStateFetchedPkgbuilds {
    /// Prepare the base root for:
    /// 1. Parsing PKGBUILDs
    /// 2. Dumping CARCH
    /// 3. Working as base layer below overlay roots needed by all PKGBUILDs
    /// 
    /// This also has the following side-effects:
    /// 1. DBs are available after fetching, note this is the only time the DB
    /// would be "updated". The DBs would be "frozen" in the later whold run,
    /// to avoid partial update during builds.
    pub(crate) fn try_prepare_base_root(self) 
        -> Result<WorkerStatePreparedBaseRoot> 
    {
        let baseroot = self.rootless.new_base_root(
            PATH_ROOT_SINGLE_BASE, true);
        let mut paconf = self.config.paconf.clone();
        paconf.set_option("SigLevel", Some("Never"));
        baseroot.prepare_layout(&paconf)?;
        self.rootless.bootstrap_root(
            &baseroot, ["base", "base-devel"], true)?;
        Ok(WorkerStatePreparedBaseRoot { 
            config: self.config,
            rootless: self.rootless,
            baseroot
        })
    }
}

pub(crate) struct WorkerStatePreparedBaseRoot {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    baseroot: BaseRoot,
}

impl WorkerStatePreparedBaseRoot {
    /// Dump CARCH from base root, in case user has set architecture to `auto`
    /// or `any` (or hasn't set any). This is needed later to filter PKGBUILDs
    /// infos by architecture.
    pub(crate) fn try_dump_arch(mut self) -> Result<WorkerStateDumpedArch> {
        let get_arch = match &self.config.arch {
            Architecture::Other(arch) => 
                match arch.to_lowercase().as_str() 
            {
                "auto" | "any" => true,
                _ => false
            },
            _ => false
        };
        if get_arch {
            log::warn!("Architecture is 'auto' or 'any', dumping arch from \
                makepkg.conf");
            self.baseroot.create_file_with_content("etc/makepkg.conf", 
                    &self.config.mpconf)?;
            let arch = 
                self.rootless.dump_arch_in_root(&self.baseroot)?;
            log::info!("Dumped arch '{}' from makepkg.conf in secured root",
                arch);
            self.config.arch = arch;
        }
        Ok(WorkerStateDumpedArch { 
            config: self.config, 
            rootless: self.rootless, 
            baseroot: self.baseroot })
    }
}

pub(crate) struct WorkerStateDumpedArch {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    baseroot: BaseRoot,
}

impl WorkerStateDumpedArch {
    /// Dump and parse PKGBUILDs in base root, the parser would be spawned in
    /// child namespace mapped to non-root user, to avoiding malicious PKGBUILDs
    /// tainting host and getting private data.
    /// 
    /// Currently this allows network access due to some PKGBUILDs needing 
    /// network to finish their PKGBUILD, but the access could be disabled.
    /// If there's conflict it's always PKGBUILDs fault being dynamic, but a
    /// per-PKGBUILD allowing network option could be introduced to work around
    /// bad PKGBUILDs.
    pub(crate) fn try_parse_pkgbuilds(mut self) 
        -> Result<WorkerStateParsedPkgbuilds> 
    {
        self.config.pkgbuilds.dump(PATH_PKGBUILDS)?;
        self.rootless.complete_pkgbuilds_in_root(
            &self.baseroot, &mut self.config.pkgbuilds)?;
        log::debug!("Parsed PKGBUILDs from secured chroot: {:?}", 
            &self.config.pkgbuilds);
        Ok(WorkerStateParsedPkgbuilds { 
            config: self.config, 
            rootless: self.rootless, 
            baseroot: self.baseroot })
    }
}

pub(crate) struct WorkerStateParsedPkgbuilds {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    baseroot: BaseRoot,
}

impl WorkerStateParsedPkgbuilds {
    /// Calculate builg plan with current DBs and PKGBUILDs info, then cache
    /// all dependent packages. The cache only happens once, as early as
    /// possible, right after DBs, PKGBUILDs info and arch are available, before
    /// any possibly time-taking business. This ensures all of our PKGBUILDs 
    /// could be built against the latest dependencies as of recorded in DBs.
    /// 
    /// Note this could still fail, if remote repo is updated between this and
    /// DB fetching, and this really is a not-too-small time window. But two
    /// base roots would take too much time, so let's pretend that wouldn't 
    /// happen, and just bail out to restart everything if that really happens
    /// 
    /// This strategy is also error-prone, if a PKGBUILD has dependencies that
    /// are determined dynamicly, and the newer dependencies are unreachable
    /// for the existing DBs. But still, let's pretent that wouldn't happen and
    /// just bail out to restart everything if that really happens.
    pub(crate) fn try_fetch_pkgs(self) -> Result<WorkerStateFetchedPkgs> {
        let dbs = self.config.paconf.try_read_dbs()?;
        let buildplan = self.config.pkgbuilds.get_plans_with_cache(&dbs, &self.config.arch)?;
        self.rootless.cache_pkgs_for_root(&self.baseroot, &buildplan.cache)?;
        Ok(WorkerStateFetchedPkgs { 
            config: self.config, 
            rootless: self.rootless, 
            baseroot: self.baseroot,
            dbs,
            buildplan: buildplan.into()
         })
    }
}

pub(crate) struct WorkerStateFetchedPkgs {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    baseroot: BaseRoot,
    dbs: PacmanDbs,
    buildplan: BuildPlan,
}

impl WorkerStateFetchedPkgs {
    /// Fetch sources depending on the PKGBUILDs info and archtecture. Caching
    /// all git sources first into `source/git`, then cache all hashed file 
    /// sources into `source/file-*`. 
    /// 
    /// This should only take long for the first
    /// build and after a remote update. In latter runs cached sources that are
    /// intact (HEAD healthy and refs are resolvable for git sources, hash
    /// valud same for hashed file sources) would be skipped.
    /// 
    /// Currently cachead sources wouldn't be cleaned unless it's explicitly
    /// issued (different from the old `arb` behaviour in which sources would
    /// always be cleaned unless `--unclean` is specified).
    pub(crate) fn try_fetch_sources(self) -> Result<WorkerStateFetchedSources> {
        let cacheable_sources = self.config.pkgbuilds
            .get_cacheable_sources(Some(&self.config.arch));
        if ! self.config.gengmr.is_empty() {
            let mut urls = self.config.pkgbuilds.git_urls();
            urls.append(&mut cacheable_sources.git_urls());
            let gmr_config = gmr_config_from_urls(&mut urls);
            log::debug!("Generated git-mirroer config: {}", &gmr_config);
            write_all_to_file_or_stdout(
                &gmr_config, &self.config.gengmr)?
        }
        cacheable_sources.cache(&self.config.gmr, &self.config.proxy,
            self.config.holdgit, self.config.lazyint)?;
        Ok(WorkerStateFetchedSources { 
            config: self.config, 
            rootless: self.rootless,
            baseroot: self.baseroot,
            dbs: self.dbs,
            buildplan: self.buildplan,
        })
    }
}

pub(crate) struct WorkerStateFetchedSources {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    baseroot: BaseRoot,
    dbs: PacmanDbs,
    buildplan: BuildPlan,
}

impl WorkerStateFetchedSources {
    /// Try to build every PKBGUILD that should be built, stages by stages (so
    /// stage-wise this is sequential), and do parallel builds for PKGBUILDs in
    /// the same stage. The "stages" come from the earlier calculation when we
    /// wanted to get the list of packages to cache.
    /// 
    /// In any scenario, if the build plan would need to be re-calculated, it
    /// shall be, then we would go from bottom to top again. But note in that
    /// case dependent pakcages wouldn't be updated again, in the hope that 
    /// PKGBUILDs would behave well and do not introduce different dependencies.
    /// 
    /// PKGBUILD that matches the following conditions would need to be built
    /// - Such PKGBUILD with the specific `pkgver` was never built
    /// - Such PKGBUILD with its depdency hashed was never built
    /// 
    /// Note that built PKGBUILDs would carry additionally `.[build count]
    /// .[dependency hash]` in their pkgver as suffix, this is currently 
    /// implemented naively as follows:
    /// 1. As we store package files in `pkgs/PKGBUILD/[name]/[version]/[hash]`,
    /// the `[build count]` is simply the count of first package files inside the
    /// `[version]` folder recursively (note the version here is the original
    /// `pkgver`, without our local suffix)
    /// 2. The `[dependency hash]` is the hash of a byte array that's assembled
    /// with every direct dependency's `sha256sum`. `sha256sum` is favored based
    /// on the result got in 
    /// `https://github.com/7Ji/alnopm/blob/2ee446963bca4fee32f110dca015efda32a4d3a0/examples/hashstat.rs`
    /// . As of writing every package in every DB in official Arch and ALARM
    /// and any third party has `sha256sum`. Every package in every DB in 
    /// official Arch and ALARM has `pgpsig`, but not every one in third party
    /// does. ALARM official all have `md5sum`, but Arch official ~50%.
    pub(crate) fn try_build(mut self) -> Result<WorkerStateBuilt> {
        loop {
            let mut built: usize = 0;
            let mut updated: usize = 0;
            let mut stages: usize = 0;
            for stage in self.buildplan.stages.iter() {
                stages += 1;
                let results: Vec<Result<(String, PlainVersion, bool)>> = 
                    stage.build.par_iter().map(|method|
                {
                    let pkgbuild = 
                        self.config.pkgbuilds.get_pkgbuild(&method.pkgbuild)?;
                    // pkgbuild.try_build();
                    // if pkgbuild
                    Ok((method.pkgbuild.clone(), Default::default(), false))
                }).collect();
                for result in results {
                    let (pkgbuild, version, 
                        built_this) = result?;
                    if built_this { built += 1 }
                    if self.config.pkgbuilds.update_pkgbuild_version(
                        &pkgbuild, version)? 
                    {
                        if ! built_this {
                            log::error!("PKGBUILD {} updated version \
                                but was not built", pkgbuild);
                            return Err(Error::BrokenPKGBUILDs(
                                                    vec![pkgbuild]))
                        }
                        updated += 1;
                    }
                }
                if updated > 0 {
                    log::warn!("Some PKGBUILD pkgver updated, re-planning...");
                    self.buildplan = self.config.pkgbuilds.get_plans(
                        &self.dbs, &self.config.arch)?;
                    // self.rootless.cache_pkgs_for_root(&self.root, 
                    //     &self.buildplan.cache)?;
                    break
                }
            }
            log::info!(
                "Built {}/{} stages, {}/{} PKGBUILDs ({} pkgver updated)", 
                    stages, self.buildplan.stages.len(), 
                    built, self.config.pkgbuilds.len(),
                    updated);
            if built == 0 { // No new package built, break now
                log::info!("Build plan properly finished, all jobs done");
                // log::info!("")
                break
            }
            log::info!("Build plan partially finished, revisiting build plan \
                all stages...");
            // if updated > 0 {
            //     log::warn!("Some PKGBUILD pkgver updated, re-planning...");
            //     self.buildplan = self.config.pkgbuilds.get_plans(
            //         &self.dbs, &self.config.arch)?;
            // }
        }
        Ok(WorkerStateBuilt { 
            config: self.config })
    }
}

pub(crate) struct WorkerStateBuilt {
    config: RuntimeConfig,
}

impl WorkerStateBuilt {
    pub(crate) fn try_release(self) -> Result<WorkerStateReleased> {
        Ok(WorkerStateReleased {})
    }
}

pub(crate) struct WorkerStateReleased {

}