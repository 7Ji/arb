use std::{collections::BTreeMap, ffi::OsString, fmt::Display, io::{BufRead, BufReader, Write}, path::{Path, PathBuf}};

use alnopm::Db;

use crate::{constant::PATH_PACMAN_SYNC, filesystem::{dir_entry_checked, file_create_checked, file_open_checked, read_dir_checked}, logfile::LogFileBuilder, rootless::BrokerPayload, Error, Result};

type ConfigSection =  BTreeMap<String, Option<String>>;

#[derive(Default, Debug, Clone)]
pub(crate) struct PacmanConfig {
    options: ConfigSection,
    repos: BTreeMap<String, ConfigSection>,
}

impl PacmanConfig {
    pub(crate) fn try_read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = file_open_checked(&path)?;
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
        log::debug!("All pacman.conf sections: {:?}", sections);
        let mut config = Self::default();
        let mut repo_core = None;
        let mut repo_extra = None;
        let mut repo_multilib = None;
        for (key, value) in sections {
            match key.as_ref() {
                "options" => config.options = value,
                "core" => repo_core = Some((key, value)),
                "extra" => repo_extra = Some((key, value)),
                "multilib" => repo_multilib = Some((key, value)),
                _ => ()
            }
        }
        for repo in 
            [repo_core, repo_extra, repo_multilib] 
        {
            if let Some((key, value)) 
                = repo 
            {
                if config.repos.insert(key, value).is_some() {
                    log::error!("Impossible: duplicated core/extra/multilib \
                        repo when parsing pacman.conf");
                    return Err(Error::ImpossibleLogic);
                }
            }
        }
        log::debug!("Read pacman config: {:?}", config);
        Ok(config)
    }

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

    pub(crate) fn retain_official_repos(&mut self) {
        self.repos.retain(|key, _|
            match key.as_str() {
                "core" | "extra" | "multilib" => true,
                _ => false
            }
        )
    }

    pub(crate) fn try_write<P: AsRef<Path>>(&self, pacman_conf: P) 
        -> Result<()> 
    {
        let mut file = file_create_checked(&pacman_conf)?;
        if let Err(e) = file.write_fmt(format_args!("{}\n", self)) {
            log::error!("Failed to write config to file: {}", e);
            Err(e.into())
        } else {
            Ok(())
        }
    }

    pub(crate) fn try_read_dbs(&self) -> Result<PacmanDbs> {
        PacmanDbs::try_read(self)
    }

    // /// Get a hash value of DBs + Packages to install (names instead of actual
    // /// packages), this makes the following  assumption: with the same sync DBs 
    // /// (byte-to-byte-identical), and the same arguments passed to pacman with
    // /// an empty local DB, there would always be same packages installed
    // /// 
    // /// This is mostly useful to cache chroots so we don't need to re-install
    // /// 
    // /// The above assumption may break in the following situations:
    // /// 1. If 
    // pub(crate) fn hash_db_pkgs<P: AsRef<Path>>(&self, db: P, pkgs: I) -> Result<u64> {

    // }

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

pub(crate) fn try_get_install_pkgs_payload<I, S>(
    root: &Path, pkgs: I, refresh: bool
) -> Result<BrokerPayload>
where
    I: IntoIterator<Item = S>, 
    S: Into<OsString>
{
    let mut suffix = String::from("install");
    if let Some(name) = root.file_name() {
        suffix.push('-');
        suffix.push_str(&name.to_string_lossy())
    }
    let logfile: OsString = LogFileBuilder::new(
        "pacman", &suffix).try_create()?.into();
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
    payload.add_init_command_run_program(logfile, "pacman", args);
    Ok(payload)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PacmanDbs {
    dbs: BTreeMap<String, Db>,
}

impl PacmanDbs {
    pub(crate) fn try_read(config: &PacmanConfig) -> Result<Self> {
        let mut dbs = BTreeMap::new();
        for db_name in config.repos.keys() {
            let db = Db::try_from_path(
                format!("{}/{}.db", PATH_PACMAN_SYNC, db_name))?;
            if dbs.insert(db_name.clone(), db).is_some() {
                log::error!("Impossible: duplicated DB");
                return Err(Error::ImpossibleLogic)
            }
        }
        Ok(Self { dbs })
    }
}