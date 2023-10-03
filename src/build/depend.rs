use alpm;

pub(super) struct Depends (pub(super) Vec<String>);

struct DbHandle {
    alpm_handle: alpm::Alpm,
}

impl DbHandle {
    fn new<S: AsRef<str>>(root: S) -> Result<Self, ()> {
        let handle = match alpm::Alpm::new(
            root.as_ref(), "/var/lib/pacman") 
        {
            Ok(handle) => handle,
            Err(e) => {
                eprintln!("Failed to open pacman DB at root '{}': {}",
                root.as_ref(), e);
                return Err(())
            },
        };
        let content = match std::fs::read_to_string(
            "/etc/pacman.conf") 
        {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to open pacman config: {}", e);
                return Err(())
            },
        };
        let sig_level = handle.default_siglevel();
        for line in content.lines() {
            let line = line.trim();
            if ! line.starts_with('[') || ! line.ends_with(']') {
                continue   
            }
            let section = line.trim_start_matches('[')
                .trim_end_matches(']');
            if section == "options" {
                continue
            }
            match handle.register_syncdb(section, sig_level) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Failed to register repo '{}': {}", section, e);
                    return Err(())
                },
            }
        }
        if handle.syncdbs().len() == 0 {
            eprintln!("No DBs defined");
            return Err(())
        }
        Ok(DbHandle { alpm_handle: handle })
    }

    fn find_satisfier<S: AsRef<str>>(&self, dep: S) -> Option<String> {
        for db in self.alpm_handle.syncdbs() {
            if let Some(pkg) = 
                db.pkgs().find_satisfier(dep.as_ref()) 
            {
                return Some(pkg.name().into())
            }
        }
        None
    }

    fn is_installed<S: AsRef<str>>(&self, pkg: S) -> bool {
        match self.alpm_handle.localdb().pkg(pkg.as_ref()) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

impl Depends {
    fn needed(&self, db_handle: &DbHandle) -> Vec<String> {
        let mut needs = vec![];
        for dep in self.0.iter() {
            match db_handle.find_satisfier(dep) {
                Some(dep) => needs.push(dep),
                None => eprintln!("Warning: dep {} not found", dep),
            }
        }
        needs.sort_unstable();
        needs.dedup();
        needs.retain(|pkg|!db_handle.is_installed(pkg));
        needs
    }
}