use std::{collections::BTreeMap, ffi::{OsStr, OsString}, fmt::Display, fs::File, io::{BufRead, BufReader, Write}, path::Path};

use crate::{rootless::{BrokerPayload, InitCommand, RootlessHandler}, Error, Result};

type ConfigSection =  BTreeMap<String, Option<String>>;

#[derive(Default, Debug, Clone)]
pub(crate) struct PacmanConfig {
    options: ConfigSection,
    repos: BTreeMap<String, ConfigSection>,
}

impl TryFrom<&Path> for PacmanConfig {
    type Error = Error;

    fn try_from(path: &Path) -> Result<Self> {
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to open pacman config file '{}': {}",
                    path.display(), e);
                return Err(e.into())
            },
        };
        let mut title_last = String::new();
        let mut sections = Vec::new();
        let mut section_last 
            = ConfigSection::new();
        for line in BufReader::new(file).lines() {
            let line = match line {
                Ok(line) => line,
                Err(e) => {
                    log::error!("Failed to read line from config: {}", e);
                    return Err(e.into())
                },
            };
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue }
            if line.starts_with('[') { // Section title
                if line.ends_with(']') {
                    if ! title_last.is_empty() {
                        sections.push((title_last, section_last));
                    }
                    title_last = line[1..line.len()-1].into();
                    section_last = ConfigSection::new();
                }
            } else {
                if title_last.is_empty() {
                    log::error!("Pacman config contains value before any \
                        valid section");
                    return Err(Error::InvalidConfig)
                }
                match line.split_once('=') {
                    Some((key, value)) => 
                        section_last.insert(key.trim().into(), 
                            Some(value.trim().into())),
                    None => section_last.insert(line.into(), None),
                };
            }
        }
        if ! title_last.is_empty() {
            sections.push((title_last, section_last));
        }
        let mut config = Self::default();
        for (key, value) in sections {
            if key == "options" {
                config.options = value
            } else {
                config.repos.insert(key, value);
            }
        }
        Ok(config)
    }
}

impl PacmanConfig {
    pub(crate) fn set_option<S1, S2>(&mut self, key: S1, value: Option<S2>)
    where
        S1: Into<String>,
        S2: Into<String>,
    {
        self.options.insert(key.into(), value.map(|v|v.into()));
    }

    pub(crate) fn set_cache_dir<S: Into<String>>(&mut self, value: S) {
        self.set_option("CacheDir", Some(value))
    }

    pub(crate) fn set_cache_dir_here(&mut self) {
        self.set_cache_dir("pkgs/cache")
    }

    pub(crate) fn set_defaults(&mut self) {
        self.set_cache_dir_here();
        self.set_option("SigLevel", Some("Required DatabaseOptional"))
    }

    pub(crate) fn set_root<S: Into<String>>(&mut self, value: S) {
        let mut path = value.into();
        self.set_option("RootDir", Some(&path));
        let len = path.len();
        path.push_str("/var/lib/pacman/");
        self.set_option("DBPath", Some(&path));
        path.truncate(len);
        path.push_str("/var/log/pacman.log");
        self.set_option("LogFile", Some(&path));
        path.truncate(len);
        path.push_str("/etc/pacman.d/gnupg/");
        self.set_option("GPGDir", Some(&path));
        path.truncate(len);
        path.push_str("/etc/pacman.d/hooks/");
        self.set_option("HookDir", Some(&path));
    }

    pub(crate) fn to_file<P: AsRef<Path>>(&self, pacman_conf: P) -> Result<()> {
        let mut file = match File::create(&pacman_conf) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Failed to create pacman config file '{}': {}",
                    pacman_conf.as_ref().display(), e);
                return Err(e.into())
            },
        };
        if let Err(e) = file.write_fmt(format_args!("{}\n", self)) {
            log::error!("Failed to write config to file: {}", e);
            Err(e.into())
        } else {
            Ok(())
        }
    }

}

impl Display for PacmanConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "[options]")?;
        for (key, value) in self.options.iter() {
            if let Some(value) = value {
                writeln!(f, "{} = {}", key, value)?;
            } else {
                writeln!(f, "{}", key)?;
            }
        }
        for (repo_name, repo_section) in 
            self.repos.iter() 
        {
            writeln!(f, "[{}]", repo_name)?;
            for (key, value) in repo_section.iter() {
                if let Some(value) = value {
                    writeln!(f, "{} = {}", key, value)?;
                } else {
                    writeln!(f, "{}", key)?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn install_pkgs<I: IntoIterator<Item = S>, S: Into<OsString>>(
    root: &Path, pkgs: I, rootless: &RootlessHandler, refresh: bool
) -> Result<()> 
{
    let mut payload = BrokerPayload::new_with_root(root);
    let arg_sync = if refresh { "-Sy" } else { "-S" };
    let mut args = vec![
        arg_sync.into(), 
        "--config".into(), 
        root.join("etc/pacman.conf").into(),
        "--noconfirm".into(),
    ];
    for pkg in pkgs.into_iter() {
        args.push(pkg.into())
    }
    payload.init_payload.commands.push(
        InitCommand::RunProgram { program: "pacman".into(), args}
    );
    rootless.run_broker(payload.try_into_bytes()?)
}