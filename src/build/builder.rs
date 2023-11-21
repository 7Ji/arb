use std::{
        path::PathBuf,
        process::{
            Child,
            Command,
        },
        thread::sleep,
        time::Duration
    };

use crate::{
        build::dir::BuildDir,
        error::{
            Error,
            Result
        },
        filesystem::remove_dir_all_try_best,
        identity::IdentityActual,
        logfile::{
            LogFile,
            LogType,
        },
        pkgbuild::{
            PKGBUILD,
            PKGBUILDs,
        },
        root::{
            OverlayRoot,
            BootstrappingOverlayRoot,
        },
    };

enum RootState {
    None,
    Boostrapping {
        bootstrapping_root: BootstrappingOverlayRoot,
    },
    Bootstrapped {
        root: OverlayRoot,
    },
}

impl Default for RootState {
    fn default() -> Self {
        RootState::None
    }
}

enum BuildState {
    None,
    Extracting {
        child: Child,
    },
    Extracted,
    Building {
        child: Child
    },
    Built,
}

impl Default for BuildState {
    fn default() -> Self {
        BuildState::None
    }
}

struct Builder<'a> {
    pkgbuild: &'a PKGBUILD,
    builddir: BuildDir,
    temp_pkgdir: PathBuf,
    command: Command,
    tries: usize,
    root_state: RootState,
    build_state: BuildState,
    log_path: PathBuf,
}

impl <'a> Builder<'a> {
    const BUILD_MAX_TRIES: usize = 3;
    fn from_pkgbuild(pkgbuild: &'a PKGBUILD, actual_identity: &IdentityActual)
        -> Result<Self>
    {
        let builddir = BuildDir::new(&pkgbuild.base)?;
        let temp_pkgdir = pkgbuild.get_temp_pkgdir()?;
        let command = pkgbuild.get_build_command(
            actual_identity, &temp_pkgdir)?;
        let build_state = if pkgbuild.extracted {
            BuildState::Extracted
        } else {
            BuildState::None
        };
        Ok(Self {
            pkgbuild,
            builddir,
            temp_pkgdir,
            command,
            tries: 0,
            root_state: RootState::default(),
            build_state,
            log_path: PathBuf::new(),
        })
    }

    fn start_extract(&mut self, actual_identity: &IdentityActual) -> Result<()> {
        match self.pkgbuild.extractor_source(actual_identity) {
            Ok(child) => {
                log::info!("Start extracting for pkgbuild '{}'",
                    &self.pkgbuild.base);
                self.build_state = BuildState::Extracting { child };
                Ok(())
            },
            Err(e) => {
                log::error!("Failed to get extractor for pkgbuild\
                 '{}'", &self.pkgbuild.base);
                Err(e)
            },
        }
    }

    fn step_build(&mut self,  heavy_load: bool, actual_identity: &IdentityActual,
        sign: Option<&str>, jobs: &mut usize ) -> Result<()>
    {
        match &mut self.build_state {
            BuildState::None =>
                if ! heavy_load {
                    self.start_extract(actual_identity)?;
                    *jobs += 1
                },
            BuildState::Extracting { child } =>
                match child.try_wait() {
                    Ok(r) => match r {
                        Some(r) => {
                            *jobs -= 1;
                            let code = r.code();
                            if let Some(0) = code {
                                log::info!(
                                    "Successfully extracted source for \
                                    pkgbuild '{}'", &self.pkgbuild.base);
                                self.build_state = BuildState::Extracted;
                            } else {
                                log::error!("Failed to extract source for \
                                    pkgbuild '{}'", &self.pkgbuild.base);
                                return Err(Error::BadChild { pid: None, code })
                            }
                        },
                        None => (),
                    },
                    Err(e) => {
                        log::error!("Failed to wait for extractor: {}", e);
                        *jobs -= 1;
                        return Err(e.into())
                    },
                },
            BuildState::Extracted =>
                if ! heavy_load {
                    let log_file = LogFile::new(
                        LogType::Build, &self.pkgbuild.pkgid)?;
                    self.log_path = log_file.path;
                    let child = match self.command
                        .stdout(log_file.file).spawn()
                    {
                        Ok(child) => child,
                        Err(e) => {
                            log::error!("Failed to spawn builder for '{}': {}",
                                &self.pkgbuild.base, e);
                            return Err(e.into())
                        },
                    };
                    self.build_state = BuildState::Building { child };
                    self.tries += 1;
                    *jobs += 1;
                    log::info!("Start building '{}', try {} of {}",
                        &self.pkgbuild.base, self.tries, Self::BUILD_MAX_TRIES);
                },
            BuildState::Building { child } =>
                match child.try_wait() {
                    Ok(r) => match r {
                        Some(r) => {
                            *jobs -= 1;
                            log::info!(
                                "Log of building '{}' was written to '{}'",
                                &self.pkgbuild.pkgid, self.log_path.display());
                            if let Some(0) = r.code() {
                                self.pkgbuild.finish_build(actual_identity,
                                    &self.temp_pkgdir, sign)?;
                                log::info!("Successfully built '{}'",
                                    &self.pkgbuild.base);
                                self.build_state = BuildState::Built;
                            } else {
                                log::error!("Failed to build '{}'",
                                    &self.pkgbuild.base);
                                if self.tries >= Self::BUILD_MAX_TRIES {
                                    log::error!("Max retries exceeded for '{}'",
                                        &self.pkgbuild.base);
                                    return Err(Error::BuildFailure)
                                }
                                // Only needed when we want to re-extract
                                // As the destructor of builddir would delete
                                // itself when silently droppped
                                if let Err(e) = remove_dir_all_try_best(
                                    &self.builddir.path)
                                {
                                    log::error!("Failed to remove build dir \
                                        after failed build attempt");
                                    return Err(e)
                                }
                                if heavy_load {
                                    self.build_state = BuildState::None;
                                } else {
                                    self.start_extract(actual_identity)?;
                                    *jobs += 1
                                }
                            }
                        },
                        None => (),
                    },
                    Err(e) => {
                        log::error!("Failed to wait for builder: {}", e);
                        *jobs -= 1;
                        return Err(e.into())
                    },
                }
            BuildState::Built => {
                log::error!("Built status should not be met by state machine");
                return Err(Error::ImpossibleLogic)
            },
        }
        Ok(())
    }

    fn step(&mut self, heavy_load: bool, actual_identity: &IdentityActual,
            nonet: bool, sign: Option<&str>, jobs: &mut usize ) -> Result<()>
    {
        match &mut self.root_state {
            RootState::None => if ! heavy_load {
                match self.pkgbuild.get_bootstrapping_overlay_root(
                    actual_identity, nonet)
                {
                    Ok(bootstrapping_root) => {
                        log::info!("Start chroot bootstrapping for pkgbuild '{}'",
                            &self.pkgbuild.base);
                        self.root_state = RootState::Boostrapping {
                            bootstrapping_root };
                        *jobs += 1;
                    },
                    Err(e) => {
                        log::error!("Failed to get chroot bootstrapper for \
                            pkgbuild '{}'", &self.pkgbuild.base);
                        return Err(e)
                    },
                }
            },
            RootState::Boostrapping {
                bootstrapping_root }
            => match bootstrapping_root.wait_noop() {
                Ok(r) => match r {
                    Some(r) => {
                        *jobs -= 1;
                        if let Err(e) = r {
                            log::error!("Bootstrapper failed");
                            return Err(e)
                        } else {
                            let old_state =
                                std::mem::take(&mut self.root_state);
                            if let RootState::Boostrapping {
                                bootstrapping_root }
                                = old_state
                            {
                                match bootstrapping_root.wait() {
                                    Ok(root) => {
                                        self.root_state =
                                            RootState::Bootstrapped { root };
                                        log::info!("Chroot bootstrapped for \
                                            pkgbuild '{}'", &self.pkgbuild.base);
                                    },
                                    Err(e) => {
                                        log::error!("Failed to bootstrap chroot \
                                            for pkgbuild '{}'",
                                            &self.pkgbuild.base);
                                        return Err(e)
                                    },
                                }
                            } else {
                                log::error!("Status inconsistent");
                                return Err(Error::ImpossibleLogic)
                            }
                        }
                    },
                    None => (),
                },
                Err(e) => {
                    *jobs -= 1;
                    log::error!("Failed to noop wait: {}", e);
                    return Err(e)
                },
            },
            RootState::Bootstrapped { root } => {
                let _ = root;
                self.step_build(heavy_load, actual_identity, sign, jobs)?
            },
        }
        Ok(())
    }
}

fn check_heavy_load(jobs: usize, cores: usize) -> bool {
    if jobs >= cores {
        return true
    }
    if match procfs::CpuPressure::new() {
        Ok(cpu_pressure) => {
            let some = cpu_pressure.some;
            some.avg10 > 10.00 || some.avg60 > 10.00 || some.avg300 > 10.00
        },
        Err(e) => {
            log::error!("Failed to get CPU pressure: {}", e);
            true
        },
    } {
        return true
    }
    match procfs::LoadAverage::new() {
        Ok(load_avg) => {
            let max_load = (cores + 2) as f32;
            load_avg.one >= max_load ||
            load_avg.five >= max_load ||
            load_avg.fifteen >= max_load
        },
        Err(e) => {
            log::error!("Failed to get load avg: {}", e);
            true
        },
    }
}

struct Builders<'a> {
    builders: Vec<Builder<'a>>,
    actual_identity: &'a IdentityActual,
    nonet: bool,
    sign: Option<&'a str>
}

impl<'a> Builders<'a> {
    fn from_pkgbuilds(
        pkgbuilds: &'a PKGBUILDs, actual_identity: &'a IdentityActual,
        nonet: bool, sign: Option<&'a str>
    ) -> Result<Self>
    {
        let mut builders = vec![];
        for pkgbuild in pkgbuilds.0.iter() {
            if ! pkgbuild.need_build {
                continue
            }
            match Builder::from_pkgbuild(pkgbuild, actual_identity) {
                Ok(builder) => builders.push(builder),
                Err(e) => {
                    log::error!("Failed to create builder for pkgbuild");
                    return Err(e)
                },
            }
        }
        Ok(Self {
            builders,
            actual_identity,
            nonet,
            sign,
        })
    }

    fn from_pkgbuild_layer(
        pkgbuild_layer: &Vec<&'a PKGBUILD>, actual_identity: &'a IdentityActual,
        nonet: bool, sign: Option<&'a str>
    ) -> Result<Self>
    {
        let mut builders = vec![];
        for pkgbuild in pkgbuild_layer.iter() {
            if ! pkgbuild.need_build {
                continue
            }
            match Builder::from_pkgbuild(pkgbuild, actual_identity) {
                Ok(builder) => builders.push(builder),
                Err(e) => {
                    log::error!("Failed to create builder for pkgbuild: {}", e);
                    return Err(e)
                },
            }
        }
        Ok(Self {
            builders,
            actual_identity,
            nonet,
            sign,
        })
    }

    fn work(&mut self)  -> Result<()>
    {
        let cpuinfo = match procfs::CpuInfo::new() {
            Ok(cpuinfo) => cpuinfo,
            Err(e) => {
                log::error!("Failed to get cpuinfo: {}", e);
                return Err(Error::ProcError(e))
            },
        };
        let cores = cpuinfo.num_cores();
        let mut r = Ok(());
        let mut jobs = 0;
        loop {
            // let jobs_last = jobs;
            let mut finished = None;
            for (id, builder) in
                self.builders.iter_mut().enumerate()
            {
                let heavy_load = check_heavy_load(jobs, cores);
                match builder.step(heavy_load, self.actual_identity, self.nonet,
                                    self.sign, &mut jobs)
                {
                    Ok(_) => if let BuildState::Built = builder.build_state {
                        finished = Some(id);
                        break
                    },
                    Err(e) => {
                        r = Err(e);
                        finished = Some(id);
                    },
                }
                if heavy_load {
                    sleep(Duration::from_secs(1))
                }
            }
            if let Some(id) = finished {
                let builder = self.builders.swap_remove(id);
                log::info!("Finished builder for PKGBUILD '{}'",
                    &builder.pkgbuild.base);
            }
            if self.builders.is_empty() {
                break
            }
            sleep(Duration::from_millis(100))
            // if jobs > jobs_last && jobs - jobs_last > 1 {
            //     sleep(Duration::from_secs(5))
            // } else if check_heavy_load(jobs, cores) {
            //     sleep(Duration::from_secs(15))
            // } else {

            // }
        }
        if jobs > 0 {
            log::error!("Jobs count is not 0 ({}) at the end", jobs);
            r = Err(Error::ImpossibleLogic);
        }
        r
    }
}

pub(super) fn build_any_needed(
    pkgbuilds: &PKGBUILDs,  actual_identity: &IdentityActual,
    nonet: bool, sign: Option<&str>
) -> Result<()>
{
    Builders::from_pkgbuilds(pkgbuilds, actual_identity, nonet, sign)?
        .work()?;
    Ok(())
}

pub(super) fn build_any_needed_layer(
    pkgbuild_layer: &Vec<&PKGBUILD>,  actual_identity: &IdentityActual,
    nonet: bool, sign: Option<&str>
) -> Result<()>
{
    Builders::from_pkgbuild_layer(pkgbuild_layer, actual_identity, nonet, sign)?
        .work()?;
    Ok(())
}