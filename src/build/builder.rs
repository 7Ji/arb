use std::{path::PathBuf, process::{Command, Child}, fs::{remove_dir_all, create_dir_all}};

use crate::{roots::{OverlayRoot, BootstrappingOverlayRoot}, identity::Identity};

use super::{pkgbuild::{PKGBUILD, PKGBUILDs}, dir::BuildDir};

enum BuilderStatus {
    None,
    Extracting {
        child: Child,
    },
    Extracted,
    Boostrapping {
        bootstrapping_root: BootstrappingOverlayRoot,
    },
    Bootstrapped {
        root: OverlayRoot,
    },
    Building {
        root: OverlayRoot,
        child: Child
    },
    Built,
}

impl Default for BuilderStatus {
    fn default() -> Self {
        BuilderStatus::None
    }
}

struct Builder<'a> {
    pkgbuild: &'a PKGBUILD,
    builddir: BuildDir,
    temp_pkgdir: PathBuf,
    command: Command,
    tries: usize,
    status: BuilderStatus,
}

impl <'a> Builder<'a> {
    const BUILD_MAX_TRIES: usize = 3;
    fn from_pkgbuild(
        pkgbuild: &'a PKGBUILD, actual_identity: &Identity, nonet: bool
    ) 
        -> Result<Self, ()> 
    {
        let builddir = BuildDir::new(&pkgbuild.base)?;
        let root = pkgbuild.get_overlay_root(
            actual_identity, nonet)?;
        let temp_pkgdir = pkgbuild.get_temp_pkgdir()?;
        let command = pkgbuild.get_build_command(
            actual_identity, &root, &temp_pkgdir)?;
        let status = if pkgbuild.extracted {
            BuilderStatus::Extracted
        } else {
            BuilderStatus::None
        };
        Ok(Self {
            pkgbuild,
            builddir,
            temp_pkgdir,
            command,
            tries: 0,
            status,
        })
    }

    fn start_build(&mut self, root: OverlayRoot) -> Result<(), ()> {
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
        self.status = BuilderStatus::Building { root, child };
        self.tries += 1;
        println!("Start building '{}', try {} of {}", &self.pkgbuild.base, 
            self.tries, Self::BUILD_MAX_TRIES);
        Ok(())
    }

    fn work(&mut self, heavy_load: bool, actual_identity: &Identity, 
            nonet: bool, sign: Option<&str> ) -> Result<(), ()> 
    {
        match &mut self.status {
            BuilderStatus::None => if !heavy_load {
                match self.pkgbuild.extractor_source(actual_identity) {
                    Ok(child) => {
                        println!("Start extracting for pkgbuild '{}'", 
                            &self.pkgbuild.base);
                        self.status = BuilderStatus::Extracting { child }
                    },
                    Err(_) => {
                        eprintln!("Failed to get extractor for pkgbuild '{}'", 
                            &self.pkgbuild.base);
                        return Err(())
                    },
                }
            },
            BuilderStatus::Extracting { child } => 
                match child.try_wait() {
                    Ok(r) => match r {
                        Some(r) => 
                            if let Some(0) = r.code() {
                                println!("Successfully extracted source for \
                                    pkgbuild '{}'", &self.pkgbuild.base);
                                self.status = BuilderStatus::Extracted
                            } else {
                                eprintln!("Failed to extract source for \
                                    pkgbuild '{}'", &self.pkgbuild.base);
                                return Err(())
                            },
                        None => (),
                    },
                    Err(e) => {
                        eprintln!("Failed to wait for extractor: {}", e);
                        return Err(())
                    },
                },
            BuilderStatus::Extracted => if !heavy_load {
                match self.pkgbuild.get_bootstrapping_overlay_root(
                    actual_identity, nonet) 
                {
                    Ok(bootstrapping_root) => {
                        println!("Start chroot bootstrapping for pkgbuild '{}'",
                            &self.pkgbuild.base);
                        self.status = BuilderStatus::Boostrapping { 
                            bootstrapping_root }
                    },
                    Err(_) => {
                        eprintln!("Failed to get chroot bootstrapper for \
                            pkgbuild '{}'", &self.pkgbuild.base);
                        return Err(())
                    },
                }
            },
            BuilderStatus::Boostrapping { 
                bootstrapping_root } 
            => match bootstrapping_root.wait_noop() {
                Ok(r) => match r {
                    Some(r) => match r {
                        Ok(_) => {
                            let old_status = 
                                std::mem::take(&mut self.status);
                            if let BuilderStatus::Boostrapping { 
                                bootstrapping_root } 
                                = old_status 
                            {
                                match bootstrapping_root.wait() {
                                    Ok(root) => {
                                        self.status = 
                                           BuilderStatus::Bootstrapped { root };
                                        println!("Chroot bootstrapped for \
                                            pkgbuild '{}'", &self.pkgbuild.base)
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
                        },
                        Err(_) => {
                            eprintln!("Bootstrapper failed");
                            return Err(())
                        },
                    },
                    None => (),
                },
                Err(_) => return Err(()),
            },
            BuilderStatus::Bootstrapped { root: _ }
            => if ! heavy_load {
                if let BuilderStatus::Bootstrapped { root } 
                    = std::mem::take(&mut self.status)
                {
                    self.start_build(root)?
                } else {
                    eprintln!("Status inconsistent");
                    return Err(())
                }
            }
            BuilderStatus::Building { root: _, 
                child } => match child.try_wait() 
            {
                Ok(r) => match r {
                    Some(r) => {
                        println!("Log of building '{}':", &self.pkgbuild.base);
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
                            self.status = BuilderStatus::Built
                        } else {
                            eprintln!("Failed to build '{}'", 
                                &self.pkgbuild.base);
                            if self.tries >= Self::BUILD_MAX_TRIES {
                                eprintln!("Max retries exceeded for '{}'", 
                                    &self.pkgbuild.base);
                                return Err(())
                            }
                            if let BuilderStatus::Building { 
                                root, child: _ } 
                                = std::mem::take(&mut self.status)
                            {
                                if heavy_load {
                                    self.status = 
                                        BuilderStatus::Bootstrapped { root }
                                } else {
                                    self.start_build(root)?
                                }
                            } else {
                                eprintln!("Status inconsistent");
                                return Err(())
                            }
                        }
                    },
                    None => (),
                },
                Err(e) => {
                    eprintln!("Failed to wait for builder: {}", e);
                    return Err(())
                },
            },
            BuilderStatus::Built => {
                eprintln!("Built status should not be met by state machine");
                return Err(())
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

struct Builders<'a> {
    builders: Vec<Builder<'a>>,
    actual_identity: &'a Identity, 
    nonet: bool, 
    sign: Option<&'a str>
}

impl<'a> Builders<'a> {
    fn from_pkgbuilds(
        pkgbuilds: &'a PKGBUILDs, actual_identity: &'a Identity, 
        nonet: bool, sign: Option<&'a str>
    ) -> Result<Self, ()> 
    {
        prepare_pkgdir()?;
        let mut builders = vec![];
        for pkgbuild in pkgbuilds.0.iter() {
            if ! pkgbuild.need_build {
                continue
            }
            match Builder::from_pkgbuild(pkgbuild, actual_identity, nonet) {
                Ok(builder) => builders.push(builder),
                Err(_) => return Err(()),
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
        while self.builders.len() > 0 {
            let mut finished = None;
            let heavy_load = match procfs::LoadAverage::new() {
                Ok(load_avg) => load_avg.one >= cores as f32,
                Err(e) => {
                    eprintln!("Failed to get load avg: {}", e);
                    true
                },
            };
            for (id, builder) in 
                self.builders.iter_mut().enumerate() 
            {
                match builder.work(heavy_load, self.actual_identity, self.nonet,
                                    self.sign) 
                {
                    Ok(_) => if let BuilderStatus::Built = builder.status {
                        finished = Some(id);
                        break
                    },
                    Err(_) => {
                        bad = true;
                        finished = Some(id);
                    },
                }
            }
            if let Some(id) = finished {
                let builder = self.builders.swap_remove(id);
                println!("Finished builder for PKGBUILD '{}'", 
                    &builder.pkgbuild.base);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if bad { Err(()) } else { Ok(()) }
    }
}

pub(super) fn build_any_needed(
    pkgbuilds: &PKGBUILDs,  actual_identity: &Identity, 
    nonet: bool, sign: Option<&str>
) -> Result<(), ()>
{
    Builders::from_pkgbuilds(pkgbuilds, actual_identity, nonet, sign)?
        .work()?;
    Ok(())
}