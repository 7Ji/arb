use std::{io::{stdout, Write}, iter::once, path::Path};

use pkgbuild::Architecture;

// Worker is a finite state machine
use crate::{cli::ActionArgs, config::{PersistentConfig, RuntimeConfig}, io::write_all_to_file_or_stdout, git::gmr_config_from_urls, rootless::{Root, RootlessHandler}, Error, Result};

pub(crate) struct WorkerStateReadConfig {
    config: PersistentConfig
}

impl WorkerStateReadConfig {
    pub(crate) fn try_new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self { config: PersistentConfig::try_read(path)? })
    }

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
    pub(crate) fn try_prepare_layout(mut self) 
        -> Result<WorkerStatePreparedLayout>
    {
        self.rootless.run_action_no_payload(
            "rm-rf", once("build"))?;
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
    pub(crate) fn try_prepare_base_root(self) 
        -> Result<WorkerStatePreparedBaseRoot> 
    {
        let root = self.rootless.new_root(
            "build/root.single.base", true);
        let mut paconf = self.config.paconf.clone();
        paconf.set_option("SigLevel", Some("Never"));
        root.prepare_layout(&paconf)?;
        self.rootless.bootstrap_root(
            &root, once("base-devel"), true)?;
        Ok(WorkerStatePreparedBaseRoot { 
            config: self.config,
            rootless: self.rootless,
            root 
        })
    }
}

pub(crate) struct WorkerStatePreparedBaseRoot {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    root: Root,
}

impl WorkerStatePreparedBaseRoot {
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
            self.root.create_file_with_content("etc/makepkg.conf", 
                    &self.config.mpconf)?;
            let arch = 
                self.rootless.dump_arch_in_root(&self.root)?;
            log::info!("Dumped arch '{}' from makepkg.conf in secured root",
                arch);
            self.config.arch = arch;
        }
        Ok(WorkerStateDumpedArch { 
            config: self.config, 
            rootless: self.rootless, 
            root: self.root })
    }
}

pub(crate) struct WorkerStateDumpedArch {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    root: Root,
}

impl WorkerStateDumpedArch {
    pub(crate) fn try_parse_pkgbuilds(mut self) 
        -> Result<WorkerStateParsedPkgbuilds> 
    {
        self.config.pkgbuilds.dump("build/PKGBUILDs")?;
        self.rootless.complete_pkgbuilds_in_root(
            &self.root, &mut self.config.pkgbuilds)?;
        log::debug!("Parsed PKGBUILDs from secured chroot: {:?}", 
            &self.config.pkgbuilds);
        Ok(WorkerStateParsedPkgbuilds { 
            config: self.config, 
            rootless: self.rootless, 
            root: self.root })
    }
}

pub(crate) struct WorkerStateParsedPkgbuilds {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    root: Root,
}

impl WorkerStateParsedPkgbuilds {
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
            root: self.root  })
    }
}

pub(crate) struct WorkerStateFetchedSources {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    root: Root,
}

impl WorkerStateFetchedSources {
    pub(crate) fn try_fetch_pkgs(self) -> Result<WorkerStateFetchedPkgs> {
        Ok(WorkerStateFetchedPkgs { 
            config: self.config, 
            rootless: self.rootless, 
            root: self.root })
    }
}

pub(crate) struct WorkerStateFetchedPkgs {
    config: RuntimeConfig,
    rootless: RootlessHandler,
    root: Root,
}

impl WorkerStateFetchedPkgs {
    pub(crate) fn try_build(self) -> Result<WorkerStateBuilt> {
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