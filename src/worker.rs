use std::{io::Write, path::Path};

// Worker is a finite state machine
use crate::{cli::ActionArgs, config::{PersistentConfig, RuntimeConfig}, rootless::RootlessHandler, Error, Result};

#[derive(Default)]
pub(crate) enum WorkerState {
    #[default]
    None,
    ReadConfig {
        config: PersistentConfig
    },
    MergedConfig {
        config: RuntimeConfig
    },
    PreparedLayout {
        config: RuntimeConfig
    },
    FetchedPkgbuilds {
        config: RuntimeConfig
    },
    PreparedRootless {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    ParsedPkgbuilds {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    FetchedSources {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    FetchedPkgs {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    MadeBaseChroot {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    MadeChroots {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    Built {
        config: RuntimeConfig,
        rootless: RootlessHandler,
    },
    Released
}


impl WorkerState {
    fn get_state_str(&self) -> &'static str {
        match self {
            WorkerState::None => "none",
            WorkerState::ReadConfig { config: _ } => "read config",
            WorkerState::MergedConfig { config: _ } => "merged config",
            WorkerState::PreparedLayout { config: _ } => "prepared layout",
            WorkerState::FetchedPkgbuilds { config: _ }=> "fetched PKGBUILDs",
            WorkerState::PreparedRootless { config: _, rootless: _ } => "prepared rootless",
            WorkerState::ParsedPkgbuilds { config: _, rootless: _ } => "parsed PKGBUILDs",
            WorkerState::FetchedSources { config: _, rootless: _ } => "fetched sources",
            WorkerState::FetchedPkgs { config: _, rootless: _ } => "fetched pkgs",
            WorkerState::MadeBaseChroot { config: _, rootless: _ } => "made base chroot",
            WorkerState::MadeChroots { config: _, rootless: _} => "made chroots",
            WorkerState::Built { config: _, rootless: _ }  => "built",
            WorkerState::Released => "released",
        }
    }

    fn get_illegal_state(&self) -> Error {
        Error::IllegalWorkerState(self.get_state_str())
    }

    pub(crate) fn new() -> Self {
        Self::None
    }

    pub(crate) fn read_config<P: AsRef<Path>>(self, path_config: P) 
        -> Result<Self> 
    {
        if let Self::None = self {
            let config = 
                PersistentConfig::try_from(path_config.as_ref())?;
            Ok(Self::ReadConfig { config })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn merge_config(self, args: ActionArgs) -> Result<Self> {
        if let Self::ReadConfig { config } = self {
            let config = RuntimeConfig::try_from((args, config))?;
            if config.pkgbuilds.is_empty() { 
                log::error!("No PKGBUILDs defined");
                return Err(Error::InvalidConfig)
            }
            if ! config.gengmr.is_empty() {
                let gmr_config = config.pkgbuilds.gengmr();
                log::info!("Generated git-mirroer config: {}", &gmr_config);
                if config.gengmr != "-" {
                    std::fs::File::create(&config.gengmr)?
                        .write_all(gmr_config.as_bytes())?
                }
            }
            Ok(Self::MergedConfig { config })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn prepare_layout(self) -> Result<Self> {
        if let Self::MergedConfig { mut config } = self {
            crate::filesystem::prepare_layout()?;
            config.paconf.set_cache_dir_here();
            config.paconf.to_file("build/pacman.cache.conf")?;
            Ok(Self::PreparedLayout { config })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn fetch_pkgbuilds(self) -> Result<Self> {
        if let Self::PreparedLayout { mut config }= self {
            config.pkgbuilds.sync(&config.gmr, &config.proxy, config.holdpkg)?;
            config.pkgbuilds.complete()?;
            Ok(Self::FetchedPkgbuilds { config })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn prepare_rootless(self) -> Result<Self> {
        if let Self::FetchedPkgbuilds { config } = self {
            let rootless = RootlessHandler::try_new()?;
            Ok(Self::PreparedRootless { config, rootless })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn parse_pkgbuilds(self) -> Result<Self> {
        if let Self::PreparedRootless { config, rootless } = self {
            config.pkgbuilds.dump("build/PKGBUILDs")?;
            rootless.new_root("build/root_base_pkgbuild_parser");

            // let pkgbuilds = 
            // rootless.run_action("read_pkgbuilds", config.pkgbuilds.entries.iter())
            Ok(Self::ParsedPkgbuilds { config, rootless })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn fetch_sources(self) -> Result<Self> {
        if let Self::ParsedPkgbuilds { config, rootless }= self {
            Ok(Self::FetchedSources { config, rootless })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn fetch_pkgs(self) -> Result<Self> {
        if let Self::FetchedSources { config, rootless } = self {
            Ok(Self::FetchedPkgs { config, rootless })
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn make_base_chroot(self) -> Result<Self> {
        if let Self::FetchedPkgs { config, rootless } = self {
            Ok(Self::MadeBaseChroot { config, rootless } )
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn make_chroots(self) -> Result<Self> {
        if let Self::MadeBaseChroot { config, rootless } = self {
            Ok(Self::MadeChroots { config, rootless} )
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn build(self) -> Result<Self> {
        if let Self::MadeChroots { config, rootless } = self {
            Ok(Self::Built { config, rootless } )
        } else {
            Err(self.get_illegal_state())
        }
    }

    pub(crate) fn release(self) -> Result<Self> {
        if let Self::Built { config, rootless } = self {
            Ok(Self::Released)
        } else {
            Err(self.get_illegal_state())
        }
    }
}