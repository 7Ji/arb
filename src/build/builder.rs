use std::{path::{PathBuf, Path}, process::{Command, Child}, fs::{File, remove_dir_all, create_dir_all}, io::{Read, stdout, Write}, thread::JoinHandle};

use crate::{roots::{OverlayRoot, BootstrappingOverlayRoot}, identity::Identity, filesystem::remove_dir_recursively};

use super::{pkgbuild::{PKGBUILD, PKGBUILDs}, dir::BuildDir};

use super::sign::sign_pkgs;

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
    // Built OverlayRoot),
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
                        println!("End of log for building '{}':",
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
    // _pkgbuilds: &'a PKGBUILDs,
    builders: Vec<Builder<'a>>,
}

impl<'a> Builders<'a> {
    // fn wait_noop(&mut self, actual_identity: &Identity, sign: Option<&str>) 
    //     -> bool 
    // {
    //     let mut bad = false;
    //     loop {
    //         let mut finished = None;
    //         for (id, builder) in 
    //             self.0.iter_mut().enumerate() 
    //         {
    //             match builder.child.try_wait() {
    //                 Ok(status) => match status {
    //                     Some(_) => {
    //                         finished = Some(id);
    //                         break
    //                     },
    //                     None => continue,
    //                 }
    //                 Err(e) => { // Kill bad child
    //                     eprintln!("Failed to wait for child: {}", e);
    //                     if let Err(e) = builder.child.kill() {
    //                         eprintln!("Failed to kill child: {}", e);
    //                     }
    //                     finished = Some(id);
    //                     bad = true;
    //                     break
    //                 },
    //             };
    //         }
    //         let mut builder = match finished {
    //             Some(finished) => self.0.swap_remove(finished),
    //             None => break, // No child waitable
    //         };
    //         println!("Log of building '{}':", &builder.pkgbuild.pkgid);
    //         if file_to_stdout(&builder.log_path).is_err() {
    //             println!("Warning: failed to read log to stdout, \
    //                 you could still manually check the log file '{}'",
    //                 builder.log_path.display())
    //         }
    //         println!("End of Log of building '{}'", &builder.pkgbuild.pkgid);
    //         if builder.pkgbuild.remove_build().is_err() {
    //             eprintln!("Failed to remove build dir");
    //             bad = true;
    //         }
    //         match builder.child.wait() {
    //             Ok(status) => {
    //                 match status.code() {
    //                     Some(code) => {
    //                         if code == 0 {
    //                             if builder.pkgbuild.build_finish(
    //                                 actual_identity,
    //                                 &builder.temp_pkgdir, sign).is_err() 
    //                             {
    //                                 eprintln!("Failed to finish build for {}",
    //                                     &builder.pkgbuild.base);
    //                                 bad = true
    //                             }
    //                             continue
    //                         }
    //                         eprintln!("Bad return from builder child: {}",
    //                                     code);
    //                     },
    //                     None => eprintln!("Failed to get return code from\
    //                             builder child"),
    //                 }
    //             },
    //             Err(e) => {
    //                 eprintln!("Failed to get child output: {}", e);
    //                 bad = true;
    //             },
    //         };
    //         if builder.tries >= Self::BUILD_MAX_TRIES {
    //             eprintln!("Max retries met for building {}, giving up",
    //                 &builder.pkgbuild.base);
    //             if let Err(e) = remove_dir_all(
    //                 &builder.temp_pkgdir
    //             ) {
    //                 eprintln!("Failed to remove temp pkg dir for failed \
    //                         build: {}", e);
    //                 bad = true
    //             }
    //             continue
    //         }
    //         if builder.pkgbuild.extract_source(actual_identity).is_err() {
    //             eprintln!("Failed to re-extract source to rebuild");
    //             bad = true;
    //             continue
    //         }
    //         let log_file = match File::create(&builder.log_path) {
    //             Ok(log_file) => log_file,
    //             Err(e) => {
    //                 eprintln!("Failed to create log file: {}", e);
    //                 continue
    //             },
    //         };
    //         builder.tries += 1;
    //         builder.child = match builder.command.stdout(log_file).spawn() {
    //             Ok(child) => child,
    //             Err(e) => {
    //                 eprintln!("Failed to spawn child: {}", e);
    //                 bad = true;
    //                 continue
    //             },
    //         };
    //         self.0.push(builder)
    //     }
    //     bad
    // }

    fn from_pkgbuilds(
        pkgbuilds: &'a PKGBUILDs, actual_identity: &Identity, 
        nonet: bool, sign: Option<&str>
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
        })
    }

    fn finish(&self, actual_identity: &Identity, sign: Option<&str>) {
        // let thread_cleaner =
        //     thread::spawn(|| Self::remove_builddir());
        // println!("Finishing building '{}'", &self.pkgid);
        // if self.pkgdir.exists() {
        //     if let Err(e) = remove_dir_all(&self.pkgdir) {
        //         eprintln!("Failed to remove existing pkgdir: {}", e);
        //         return Err(())
        //     }
        // }
        // if let Some(key) = sign {
        //     Self::sign_pkgs(actual_identity, temp_pkgdir, key)?;
        // }
        // if let Err(e) = rename(&temp_pkgdir, &self.pkgdir) {
        //     eprintln!("Failed to rename temp pkgdir '{}' to persistent pkgdir \
        //         '{}': {}", temp_pkgdir.display(), self.pkgdir.display(), e);
        //     return Err(())
        // }
        // self.link_pkgs()?;
        // println!("Finished building '{}'", &self.pkgid);
        // let _ = thread_cleaner.join()
        //     .expect("Failed to join cleaner thread");
        // Ok(())
    }

    fn work(&mut self, actual_identity: &Identity, nonet: bool, sign: Option<&str>) 
        -> Result<(), ()> 
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
                match builder.work(heavy_load, actual_identity, nonet, sign) {
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
        Ok(())
    }
}

pub(super) fn build_any_needed(
    pkgbuilds: &PKGBUILDs,  actual_identity: &Identity, 
    nonet: bool, sign: Option<&str>
) -> Result<(), ()>
{
    let mut builders = 
        Builders::from_pkgbuilds(pkgbuilds, actual_identity, nonet, sign)?;
    builders.work(actual_identity, nonet, sign)?;
    Ok(())
}