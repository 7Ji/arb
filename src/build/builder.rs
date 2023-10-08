use std::{path::PathBuf, process::{Command, Child}, fs::{remove_dir_all, create_dir_all}, thread::sleep, time::Duration};

use crate::{roots::{OverlayRoot, BootstrappingOverlayRoot}, identity::IdentityActual, filesystem::remove_dir_all_try_best};

use super::{pkgbuild::{PKGBUILD, PKGBUILDs}, dir::BuildDir};

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
    build_state: BuildState
}

impl <'a> Builder<'a> {
    const BUILD_MAX_TRIES: usize = 3;
    fn from_pkgbuild(pkgbuild: &'a PKGBUILD, actual_identity: &IdentityActual) 
        -> Result<Self, ()> 
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
        })
    }

    fn start_extract(&mut self, actual_identity: &IdentityActual) -> Result<(), ()> {
        match self.pkgbuild.extractor_source(actual_identity) {
            Ok(child) => {
                println!("Start extracting for pkgbuild '{}'", 
                    &self.pkgbuild.base);
                self.build_state = BuildState::Extracting { child };
                Ok(())
            },
            Err(_) => {
                eprintln!("Failed to get extractor for pkgbuild\
                 '{}'", &self.pkgbuild.base);
                Err(())
            },
        }
    }

    fn step_build(&mut self,  heavy_load: bool, actual_identity: &IdentityActual, 
        sign: Option<&str>, jobs: &mut usize ) -> Result<(), ()> 
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
                            if let Some(0) = r.code() {
                                println!(
                                    "Successfully extracted source for \
                                    pkgbuild '{}'", &self.pkgbuild.base);
                                self.build_state = BuildState::Extracted;
                            } else {
                                eprintln!("Failed to extract source for \
                                    pkgbuild '{}'", &self.pkgbuild.base);
                                return Err(())
                            }
                        },
                        None => (),
                    },
                    Err(e) => {
                        eprintln!("Failed to wait for extractor: {}", e);
                        *jobs -= 1;
                        return Err(())
                    },
                },
            BuildState::Extracted => 
                if ! heavy_load {
                    let log_file = self.builddir.get_log_file()?;
                    let child = match self.command
                        .stdout(log_file).spawn() 
                    {
                        Ok(child) => child,
                        Err(e) => {
                            eprintln!("Failed to spawn builder for '{}': {}", 
                                &self.pkgbuild.base, e);
                            return Err(())
                        },
                    };
                    self.build_state = BuildState::Building { child };
                    self.tries += 1;
                    *jobs += 1;
                    println!("Start building '{}', try {} of {}", 
                        &self.pkgbuild.base, self.tries, Self::BUILD_MAX_TRIES);
                    self.builddir.hint_log()
                },
            BuildState::Building { child } => 
                match child.try_wait() {
                    Ok(r) => match r {
                        Some(r) => {
                            *jobs -= 1;
                            println!("Log of building '{}':", 
                                &self.pkgbuild.base);
                            if self.builddir.read_log().is_err() {
                                eprintln!("Failed to read log")
                            }
                            println!("End of log for building '{}'",
                                &self.pkgbuild.base);
                            if let Some(0) = r.code() {
                                self.pkgbuild.finish_build(actual_identity, 
                                    &self.temp_pkgdir, sign)?;
                                println!("Successfully built '{}'", 
                                    &self.pkgbuild.base);
                                self.build_state = BuildState::Built;
                            } else {
                                eprintln!("Failed to build '{}'", 
                                    &self.pkgbuild.base);
                                if self.tries >= Self::BUILD_MAX_TRIES {
                                    eprintln!("Max retries exceeded for '{}'", 
                                        &self.pkgbuild.base);
                                    return Err(())
                                }
                                // Only needed when we want to re-extract
                                // As the destructor of builddir would delete
                                // itself when silently droppped
                                if remove_dir_all_try_best(
                                    &self.builddir.path).is_err() {
                                    eprintln!("Failed to remove build dir \
                                        after failed build attempt");
                                    return Err(())
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
                        eprintln!("Failed to wait for builder: {}", e);
                        *jobs -= 1;
                        return Err(())
                    },
                }
            BuildState::Built => {
                eprintln!("Built status should not be met by state machine");
                return Err(())
            },
        }
        Ok(())
    }

    fn step(&mut self, heavy_load: bool, actual_identity: &IdentityActual, 
            nonet: bool, sign: Option<&str>, jobs: &mut usize ) -> Result<(), ()> 
    {
        match &mut self.root_state {
            RootState::None => if ! heavy_load {
                match self.pkgbuild.get_bootstrapping_overlay_root(
                    actual_identity, nonet) 
                {
                    Ok(bootstrapping_root) => {
                        println!("Start chroot bootstrapping for pkgbuild '{}'",
                            &self.pkgbuild.base);
                        self.root_state = RootState::Boostrapping { 
                            bootstrapping_root };
                        *jobs += 1;
                    },
                    Err(_) => {
                        eprintln!("Failed to get chroot bootstrapper for \
                            pkgbuild '{}'", &self.pkgbuild.base);
                        return Err(())
                    },
                }
            },
            RootState::Boostrapping { 
                bootstrapping_root } 
            => match bootstrapping_root.wait_noop() {
                Ok(r) => match r {
                    Some(r) => {
                        *jobs -= 1;
                        if r.is_ok() {
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
                                        println!("Chroot bootstrapped for \
                                            pkgbuild '{}'", &self.pkgbuild.base);
                                    },
                                    Err(_) => {
                                        eprintln!("Failed to bootstrap chroot \
                                            for pkgbuild '{}'", 
                                            &self.pkgbuild.base);
                                        return Err(())
                                    },
                                }
                            } else {
                                eprintln!("Status inconsistent");
                                return Err(())
                            }
                        } else  {
                            eprintln!("Bootstrapper failed");
                            return Err(())
                        }
                    },
                    None => (),
                },
                Err(_) => {
                    *jobs -= 1;
                    return Err(())
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

fn prepare_pkgdir() -> Result<(), ()> {
    let _ = remove_dir_all("pkgs/updated");
    let _ = remove_dir_all("pkgs/latest");
    if let Err(e) = create_dir_all("pkgs/updated") {
        eprintln!("Failed to create pkgs/updated: {}", e);
        return Err(())
    }
    if let Err(e) = create_dir_all("pkgs/latest") {
        eprintln!("Failed to create pkgs/latest: {}", e);
        return Err(())
    }
    Ok(())
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
            eprintln!("Failed to get CPU pressure: {}", e);
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
            eprintln!("Failed to get load avg: {}", e);
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
    ) -> Result<Self, ()> 
    {
        prepare_pkgdir()?;
        let mut builders = vec![];
        for pkgbuild in pkgbuilds.0.iter() {
            if ! pkgbuild.need_build {
                continue
            }
            match Builder::from_pkgbuild(pkgbuild, actual_identity) {
                Ok(builder) => builders.push(builder),
                Err(_) => {
                    eprintln!("Failed to create builder for pkgbuild");
                    return Err(())
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

    fn work(&mut self)  -> Result<(), ()> 
    {
        let cpuinfo = procfs::CpuInfo::new().or_else(|e|{
            eprintln!("Failed to get cpuinfo: {}", e);
            Err(())
        })?;
        let cores = cpuinfo.num_cores();
        let mut bad = false;
        let mut jobs = 0;
        loop {
            let jobs_last = jobs;
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
                    Err(_) => {
                        bad = true;
                        finished = Some(id);
                    },
                }
                if heavy_load {
                    sleep(Duration::from_secs(1))
                }
            }
            if let Some(id) = finished {
                let builder = self.builders.swap_remove(id);
                println!("Finished builder for PKGBUILD '{}'", 
                    &builder.pkgbuild.base);
            }
            if self.builders.is_empty() {
                break
            }
            if jobs > jobs_last && jobs - jobs_last > 1 {
                sleep(Duration::from_secs(5))
            } else if check_heavy_load(jobs, cores) {
                sleep(Duration::from_secs(15))
            } else {
                sleep(Duration::from_millis(100))
            }
        }
        if jobs > 0 {
            eprintln!("Jobs count is not 0 ({}) at the end", jobs);
        }
        if bad { Err(()) } else { Ok(()) }
    }
}

pub(super) fn build_any_needed(
    pkgbuilds: &mut PKGBUILDs,  actual_identity: &IdentityActual, 
    nonet: bool, sign: Option<&str>
) -> Result<(), ()>
{
    Builders::from_pkgbuilds(pkgbuilds, actual_identity, nonet, sign)?
        .work()?;
    Ok(())
}