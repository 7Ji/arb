use crate::{
        git,
        source::{
            self,
            MapByDomain,
        },
        threading::{
            self,
            wait_if_too_busy,
        },
    };
use git2::Oid;
use std::{
        collections::HashMap,
        env,
        ffi::OsString,
        fs::{
            create_dir_all,
            read_dir,
            remove_dir,
            remove_dir_all,
            remove_file,
            rename,
        },
        io::Write,
        os::unix::{
            fs::symlink,
            process::CommandExt
        },
        path::{
            PathBuf,
            Path,
        },
        process::{
            Child,
            Command, 
            Stdio
        },
        thread,
        iter::zip,
    };
use tempfile::tempdir;
use xxhash_rust::xxh3::xxh3_64;


#[derive(Clone)]
enum Pkgver {
    Plain,
    Func { pkgver: String },
}

#[derive(Clone)]
pub(crate) struct PKGBUILD {
    name: String,
    url: String,
    build: PathBuf,
    git: PathBuf,
    pkgid: String,
    pkgdir: PathBuf,
    commit: git2::Oid,
    dephash: u64,
    pkgver: Pkgver,
    extract: bool,
    sources: Vec<source::Source>,
}

impl source::MapByDomain for PKGBUILD {
    fn url(&self) -> &str {
        self.url.as_str()
    }
}

impl git::ToReposMap for PKGBUILD {
    fn url(&self) -> &str {
        self.url.as_str()
    }

    fn hash_url(&self) -> u64 {
        xxh3_64(&self.url.as_bytes())
    }

    fn path(&self) -> Option<&Path> {
        Some(&self.git.as_path())
    }
}

fn read_pkgbuilds_yaml<P>(yaml: P) -> Vec<PKGBUILD>
where
    P: AsRef<Path>
{
    let f = std::fs::File::open(yaml)
            .expect("Failed to open pkgbuilds YAML config");
    let config: HashMap<String, String> =
        serde_yaml::from_reader(f)
            .expect("Failed to parse into config");
    let mut pkgbuilds: Vec<PKGBUILD> = config.iter().map(
        |(name, url)| {
            let mut build = PathBuf::from("build");
            build.push(name);
            let git =
                PathBuf::from(format!("sources/PKGBUILD/{}", name));
            PKGBUILD {
                name: name.clone(),
                url: url.clone(),
                build,
                git,
                pkgid: String::new(),
                pkgdir: PathBuf::from("pkgs"),
                commit: Oid::zero(),
                dephash: 0,
                pkgver: Pkgver::Plain,
                extract: false,
                sources: vec![],
            }
    }).collect();
    pkgbuilds.sort_unstable_by(
        |a, b| a.name.cmp(&b.name));
    pkgbuilds
}

fn sync_pkgbuilds(pkgbuilds: &Vec<PKGBUILD>, hold: bool, proxy: Option<&str>) 
    -> Result<(), ()> 
{
    let map =
        PKGBUILD::map_by_domain(pkgbuilds);
    let repos_map =
        match git::ToReposMap::to_repos_map(
            map, "sources/PKGBUILD", None) 
    {
        Some(repos_map) => repos_map,
        None => {
            eprintln!("Failed to convert to repos map");
            return Err(())
        },
    };
    git::Repo::sync_mt(repos_map, git::Refspecs::MasterOnly, hold, proxy)
}

fn healthy_pkgbuild(pkgbuild: &mut PKGBUILD, set_commit: bool) -> bool {
    let repo =
        match git::Repo::open_bare(&pkgbuild.git, &pkgbuild.url, None) {
            Some(repo) => repo,
            None => {
                eprintln!("Failed to open or init bare repo {}",
                pkgbuild.git.display());
                return false
            }
        };
    if set_commit {
        match repo.get_branch_commit_id("master") {
            Some(id) => pkgbuild.commit = id,
            None => {
                eprintln!("Failed to set commit id for pkgbuild {}",
                            pkgbuild.name);
                return false
            },
        }
    }
    println!("PKGBUILD '{}' at commit '{}'", pkgbuild.name, pkgbuild.commit);
    match repo.get_pkgbuild_blob() {
        Some(_) => return true,
        None => {
            eprintln!("Failed to get PKGBUILD blob");
            return false
        },
    };
}

fn healthy_pkgbuilds(pkgbuilds: &mut Vec<PKGBUILD>, set_commit: bool) -> bool {
    for pkgbuild in pkgbuilds.iter_mut() {
        if ! healthy_pkgbuild(pkgbuild, set_commit) {
            return false;
        }
    }
    true
}

fn dump_pkgbuilds<P> (dir: P, pkgbuilds: &Vec<PKGBUILD>)
where
    P: AsRef<Path>
{
    let dir = dir.as_ref();
    for pkgbuild in pkgbuilds.iter() {
        let path = dir.join(&pkgbuild.name);
        let repo =
            git::Repo::open_bare(&pkgbuild.git, &pkgbuild.url, None)
            .expect("Failed to open repo");
        let blob = repo.get_pkgbuild_blob()
            .expect("Failed to get PKGBUILD blob");
        let mut file =
            std::fs::File::create(path)
            .expect("Failed to create file");
        file.write_all(blob.content()).expect("Failed to write");
    }
}

fn get_deps<P: AsRef<Path>> (dir: P, pkgbuilds: &Vec<PKGBUILD>) 
    -> (Vec<Vec<String>>, Vec<String>) {
    const SCRIPT: &str = include_str!("scripts/get_depends.bash");
    let children: Vec<Child> = pkgbuilds.iter().map(|pkgbuild| {
        let pkgbuild_file = dir.as_ref().join(&pkgbuild.name);
        Command::new("/bin/bash")
            .arg("-ec")
            .arg(SCRIPT)
            .arg("Depends reader")
            .arg(&pkgbuild_file)
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to spawn depends reader")
    }).collect();
    let mut pkgs_deps = vec![];
    let mut all_deps = vec![];
    for child in children {
        let output = child.wait_with_output()
            .expect("Failed to wait for child");
        let mut pkg_deps = vec![];
        for line in 
            output.stdout.split(|byte| byte == &b'\n') 
        {
            if line.len() == 0 {
                continue;
            }
            let dep = String::from_utf8_lossy(line).into_owned();
            all_deps.push(dep.clone());
            pkg_deps.push(dep);
        }
        pkgs_deps.push(pkg_deps);
    }
    all_deps.sort();
    all_deps.dedup();
    (pkgs_deps, all_deps)
}

fn install_deps(deps: &Vec<String>) -> Result<(), ()> {
    println!("Checking if needed to install {} deps: {:?}", deps.len(), deps);
    let output = Command::new("/usr/bin/pacman")
        .arg("-T")
        .args(deps)
        .output()
        .expect("Failed to run pacman to get missing deps");
    match output.status.code() {
        Some(code) => match code {
            0 => return Ok(()),
            127 => (),
            _ => {
                eprintln!(
                    "Pacman returned unexpected {} which marks fatal error",
                    code);
                return Err(())
            }
        },
        None => {
            eprintln!("Failed to get return code from pacman");
            return Err(())
        },
    }
    let mut missing_deps = vec![];
    missing_deps.clear();
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.len() == 0 {
            continue;
        }
        missing_deps.push(String::from_utf8_lossy(line).into_owned());
    }
    if missing_deps.len() == 0 {
        return Ok(())
    }
    println!("Installing {} missing deps: {:?}",
            missing_deps.len(), missing_deps);
    let mut child = Command::new("/usr/bin/sudo")
        .arg("/usr/bin/pacman")
        .arg("-S")
        .arg("--noconfirm")
        .args(&missing_deps)
        .spawn()
        .expect("Failed to run sudo pacman to install missing deps");
    let exit_status = child.wait()
        .expect("Failed to wait for child sudo pacman process");
    if match exit_status.code() {
        Some(code) => {
            if code == 0 {
                true
            } else {
                println!("Failed to run sudo pacman, return: {}", code);
                false
            }
        },
        None => false,
    } {
        println!("Successfully installed {} missing deps", missing_deps.len());
        Ok(())
    } else {
        eprintln!("Failed to install missing deps");
        Err(())
    }
}

fn calc_dep_hashes(pkgbuilds: &mut Vec<PKGBUILD>, pkgs_deps: &Vec<Vec<String>>
) {
    assert!(pkgbuilds.len() == pkgs_deps.len());
    let children: Vec<Child> = pkgs_deps.iter().map(|pkg_deps| {
        Command::new("/usr/bin/pacman")
            .arg("-Qi")
            .env("LANG", "C")
            .args(pkg_deps)
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to spawn dep info reader")
    }).collect();
    assert!(pkgbuilds.len() == children.len());
    for (pkgbuild, child) in 
        zip(pkgbuilds.iter_mut(), children) 
    {
        let output = child.wait_with_output()
            .expect("Failed to wait for child");
        pkgbuild.dephash = xxh3_64(output.stdout.as_slice());
        println!("PKGBUILD '{}' dephash is '{:016x}'", 
                pkgbuild.name, pkgbuild.dephash);
    }
}


fn check_deps<P: AsRef<Path>> (dir: P, pkgbuilds: &mut Vec<PKGBUILD>)
    -> Result<(), ()>
{
    let (pkgs_deps, all_deps) 
        = get_deps(dir, pkgbuilds);
    if all_deps.len() > 0 {
        install_deps(&all_deps)?;
    }
    calc_dep_hashes(pkgbuilds, &pkgs_deps);
    Ok(())
}

fn get_all_sources<P: AsRef<Path>> (dir: P, pkgbuilds: &mut Vec<PKGBUILD>)
    -> Option<(Vec<source::Source>, Vec<source::Source>, Vec<source::Source>)> {
    let mut sources_non_unique = vec![];
    for pkgbuild in pkgbuilds.iter_mut() {
        if let Some(sources) = source::get_sources::<P>(
            &dir.as_ref().join(&pkgbuild.name)) 
        {
            pkgbuild.sources = sources
        } else {
            return None
        }
    }
    for pkgbuild in pkgbuilds.iter() {
        for source in pkgbuild.sources.iter() {
            sources_non_unique.push(source);
        }
    }
    source::unique_sources(&sources_non_unique)
}

fn get_pkgbuilds<P>(config: P, hold: bool, noclean: bool, proxy: Option<&str>)
    -> Option<Vec<PKGBUILD>>
where
    P:AsRef<Path>
{
    let mut pkgbuilds = read_pkgbuilds_yaml(config);
    let update_pkg = if hold {
        if healthy_pkgbuilds(&mut pkgbuilds, true) {
            println!(
                "Holdpkg set and all PKGBUILDs healthy, no need to update");
            false
        } else {
            eprintln!(
                "Warning: holdpkg set, but PKGBUILDs unhealthy, need update");
            true
        }
    } else {
        true
    };
    // Should not need sort, as it's done when pkgbuilds was read
    let used: Vec<String> = pkgbuilds.iter().map(
        |pkgbuild| pkgbuild.name.clone()).collect();
    let cleaner = match noclean {
        true => None,
        false => Some(thread::spawn(move || 
                    source::remove_unused("sources/PKGBUILD", &used))),
    };
    if update_pkg {
        sync_pkgbuilds(&pkgbuilds, hold, proxy).ok()?;
        if ! healthy_pkgbuilds(&mut pkgbuilds, true) {
            eprintln!("Updating broke some of our PKGBUILDs");
            return None
        }
    }
    if let Some(cleaner) = cleaner {
        cleaner.join().expect("Failed to join PKGBUILDs cleaner thread");
    }
    Some(pkgbuilds)
}

fn extractor_source(pkgbuild: &PKGBUILD) -> Option<Child> {
    const SCRIPT: &str = include_str!("scripts/extract_sources.bash");
    create_dir_all(&pkgbuild.build)
        .expect("Failed to create build dir");
    let repo = 
        git::Repo::open_bare(&pkgbuild.git, &pkgbuild.url, None)
        .expect("Failed to open repo");
    repo.checkout_branch(&pkgbuild.build, "master").ok()?;
    source::extract(&pkgbuild.build, &pkgbuild.sources);
    let mut arg0 = OsString::from("[EXTRACTOR/");
    arg0.push(&pkgbuild.name);
    arg0.push("] /bin/bash");
    Some(Command::new("/bin/bash")
        .arg0(&arg0)
        .arg("-ec")
        .arg(SCRIPT)
        .arg("Source extractor")
        .arg(&pkgbuild.build.canonicalize()
            .expect("Failed to cannicalize build dir"))
        .spawn()
        .expect("Failed to run script"))
}

fn extract_sources(pkgbuilds: &mut [&mut PKGBUILD]) -> Result<(), ()> {
    let mut children = vec![];
    let mut bad = false;
    for pkgbuild in pkgbuilds.iter_mut() {
        if let Some(child) = extractor_source(pkgbuild) {
            children.push(child);
        } else {
            bad = true;
        }
    }
    for mut child in children {
        child.wait().expect("Failed to wait for child");
    }
    if bad { Err(()) } else { Ok(()) }
}

fn fill_all_pkgvers<P: AsRef<Path>>(dir: P, pkgbuilds: &mut Vec<PKGBUILD>)
    -> Result<(), ()> {
    let _ = remove_dir_recursively("build");
    let dir = dir.as_ref();
    let children: Vec<Child> = pkgbuilds.iter().map(|pkgbuild| 
        Command::new("/bin/bash")
            .arg("-c")
            .arg(". \"$1\"; type -t pkgver")
            .arg("Type Identifier")
            .arg(dir.join(&pkgbuild.name))
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to run script")
    ).collect();
    let mut pkgbuilds_with_pkgver_func = vec![];
    for (child, pkgbuild) in 
        zip(children, pkgbuilds.iter_mut()) 
    {
        let output = child.wait_with_output()
            .expect("Failed to wait for spanwed script");
        if output.stdout.as_slice() ==  b"function\n" {
            pkgbuilds_with_pkgver_func.push(pkgbuild);
        };
    }
    extract_sources(&mut pkgbuilds_with_pkgver_func)?;
    let children: Vec<Child> = pkgbuilds_with_pkgver_func.iter().map(
    |pkgbuild|
        Command::new("/bin/bash")
            .arg("-ec")
            .arg("srcdir=\"$1\"; cd \"$1\"; source ../PKGBUILD; pkgver")
            .arg("Pkgver runner")
            .arg(pkgbuild.build.join("src")
                .canonicalize()
                .expect("Failed to canonicalize dir"))
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to run script")
    ).collect();
    for (child, pkgbuild) in 
        zip(children, pkgbuilds_with_pkgver_func.iter_mut()) 
    {
        let output = child.wait_with_output()
            .expect("Failed to wait for child");
        pkgbuild.pkgver = Pkgver::Func { pkgver:
            String::from_utf8_lossy(&output.stdout).trim().to_string()}
    }
    Ok(())
}

fn fill_all_pkgdirs(pkgbuilds: &mut Vec<PKGBUILD>) {
    for pkgbuild in pkgbuilds.iter_mut() {
        let mut pkgid = format!(
            "{}-{}-{:016x}", pkgbuild.name, pkgbuild.commit, pkgbuild.dephash);
        if let Pkgver::Func { pkgver } = &pkgbuild.pkgver {
            pkgid.push('-');
            pkgid.push_str(&pkgver);
        }
        pkgbuild.pkgdir.push(&pkgid);
        pkgbuild.pkgid = pkgid;
        println!("PKGBUILD '{}' pkgid is '{}'", pkgbuild.name, pkgbuild.pkgid);
    }
}

fn extract_if_need_build(pkgbuilds: &mut Vec<PKGBUILD>) -> Result<(), ()> {
    let mut pkgbuilds_need_build = vec![];
    let mut cleaners = vec![];
    let mut bad = false;
    for pkgbuild in pkgbuilds.iter_mut() {
        let mut built = false;
        if let Ok(mut dir) = pkgbuild.pkgdir.read_dir() {
            if let Some(_) = dir.next() {
                built = true;
            }
        }
        if built { // Does not need build
            println!("Skipped already built '{}'",
                pkgbuild.pkgdir.display());
            if pkgbuild.extract {
                let dir = pkgbuild.build.clone();
                if let Err(_) = wait_if_too_busy(
                    &mut cleaners, 30, 
                    "cleaning builddir") {
                    bad = true
                }
                cleaners.push(thread::spawn(||
                    remove_dir_recursively(dir)
                    .or(Err(()))));
                pkgbuild.extract = false;
            }
        } else {
            if ! pkgbuild.extract {
                pkgbuild.extract = true;
                pkgbuilds_need_build.push(pkgbuild);
            }
        }
    }
    if let Err(_) = extract_sources(&mut pkgbuilds_need_build) {
        bad = true
    }
    if let Err(_) = threading::wait_remaining(
        cleaners, "cleaning builddirs") 
    {
        bad = true
    }
    if bad { Err(()) } else { Ok(()) }
}

// build/*/pkg being 0111 would cause remove_dir_all() to fail, in this case
// we use our only implementation
fn remove_dir_recursively<P: AsRef<Path>>(dir: P) -> Result<(), std::io::Error>
{
    for entry in read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_symlink() && path.is_dir() {
            let er = remove_dir_recursively(&path);
            match remove_dir(&path) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Failed to remove subdir '{}' recursively: {}", 
                            path.display(), e);
                    if let Err(e) = er {
                        eprintln!("Subdir failure: {}", e)
                    }
                    return Err(e);
                },
            }
        } else {
            remove_file(&path)?
        }
    }
    Ok(())
}

fn remove_builddir() -> Result<(), std::io::Error> {
    // Go the simple way first
    match remove_dir_all("build") {
        Ok(_) => return Ok(()),
        Err(e) => eprintln!("Failed to clean: {}", e),
    }
    // build/*/pkg being 0111 would cause remove_dir_all() to fail, in this case
    // we use our only implementation
    remove_dir_recursively("build")?;
    remove_dir("build")
}

fn prepare_sources<P: AsRef<Path>>(
    dir: P,
    pkgbuilds: &mut Vec<PKGBUILD>,
    holdgit: bool,
    skipint: bool,
    noclean: bool,
    proxy: Option<&str>,
    gmr: Option<&git::Gmr>
) -> Result<(), ()> {
    let cleaner = match 
        PathBuf::from("build").exists() 
    {
        true => Some(thread::spawn(|| remove_builddir())),
        false => None,
    };
    dump_pkgbuilds(&dir, pkgbuilds);
    check_deps(&dir, pkgbuilds)?;
    let (netfile_sources, git_sources, _)
        = get_all_sources(&dir, pkgbuilds).ok_or(())?;
    source::cache_sources_mt(
        &netfile_sources, &git_sources, holdgit, skipint, proxy, gmr)?;
    if let Some(cleaner) = cleaner {
        match cleaner.join()
            .expect("Failed to join build dir cleaner thread") {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to clean build dir: {}", e);
                return Err(())
            },
        }
    }
    let cleaners = match noclean {
        true => None,
        false => Some(source::cleanup(netfile_sources, git_sources)),
    };
    fill_all_pkgvers(dir, pkgbuilds)?;
    fill_all_pkgdirs(pkgbuilds);
    extract_if_need_build(pkgbuilds)?;
    if let Some(cleaners) = cleaners {
        for cleaner in cleaners {
            cleaner.join().expect("Failed to join sources cleaner thread");
        }
    }
    Ok(())
}

fn prepare_temp_pkgdir(pkgbuild: &PKGBUILD) -> Result<PathBuf, ()> {
    let mut temp_name = pkgbuild.pkgid.clone();
    temp_name.push_str(".temp");
    let temp_pkgdir = pkgbuild.pkgdir.with_file_name(temp_name);
    match create_dir_all(&temp_pkgdir) {
        Ok(_) => Ok(temp_pkgdir),
        Err(e) => {
            eprintln!("Failed to create temp pkgdir: {}", e);
            Err(())
        },
    }
}

fn prepare_build_command(pkgbuild: &PKGBUILD, nonet: bool, temp_pkgdir: &Path) 
    -> Result<Command, ()> 
{
    let mut command = if nonet {
        let mut command = Command::new("/usr/bin/unshare");
        command.arg("--map-root-user")
            .arg("--net")
            .arg("--")
            .arg("sh")
            .arg("-c")
            .arg(format!(
                "ip link set dev lo up
                unshare --map-users={}:0:1 --map-groups={}:0:1 -- \
                    makepkg --holdver --nodeps --noextract --ignorearch", 
                unsafe {libc::getuid()}, unsafe {libc::getgid()}));
        command
    } else {
        let mut command = Command::new("/bin/bash");
        command.arg("/usr/bin/makepkg")
            .arg("--holdver")
            .arg("--nodeps")
            .arg("--noextract")
            .arg("--ignorearch");
        command
    };
    let path = env::var_os("PATH").ok_or(())?;
    let home = env::var_os("HOME").ok_or(())?;
    let pkgdest = temp_pkgdir.canonicalize().or(Err(()))?;
    command.current_dir(&pkgbuild.build)
        .arg0(format!("[BUILDER/{}] /bin/bash", pkgbuild.pkgid))
        .env("PATH", path)
        .env("HOME", home)
        .env("PKGDEST", pkgdest);
    Ok(command)
}

fn build_try(pkgbuild: &PKGBUILD, command: &mut Command, temp_pkgdir: &Path)
    -> Result<(), ()>
{
    const BUILD_MAX_TRIES: u8 = 3;
    for i in 1..BUILD_MAX_TRIES {
        println!("Building '{}', try {}/{}", 
                &pkgbuild.pkgid, i , BUILD_MAX_TRIES);
        let exit_status = command
            .spawn()
            .or(Err(()))?
            .wait()
            .or(Err(()))?;
        if let Err(e) = remove_dir_recursively(&pkgbuild.build) {
            eprintln!("Failed to remove build dir '{}': {}",
                        pkgbuild.build.display(), e);
            return Err(())
        }
        match exit_status.code() {
            Some(0) => {
                println!("Successfully built to '{}'", temp_pkgdir.display());
                return Ok(())
            },
            _ => {
                eprintln!("Failed to build to '{}'", temp_pkgdir.display());
                if let Err(e) = remove_dir_all(&temp_pkgdir) {
                    eprintln!("Failed to remove temp pkgdir '{}': {}", 
                                temp_pkgdir.display(), e);
                    return Err(())
                }
                if i == BUILD_MAX_TRIES {
                    break
                }
                let mut child = match extractor_source(&pkgbuild) {
                    Some(child) => child,
                    None => return Err(()),
                };
                if let Err(e) = child.wait() {
                    eprintln!("Failed to re-extract source for '{}': {}",
                            pkgbuild.pkgid, e);
                    return Err(())
                }
            }
        }
    }
    eprintln!("Failed to build '{}' after all tries", temp_pkgdir.display());
    Err(())
}

fn build_finish(pkgbuild: &PKGBUILD, temp_pkgdir: &Path) -> Result<(), ()> {
    println!("Finishing building '{}'", &pkgbuild.pkgid);
    if pkgbuild.pkgdir.exists() {
        if let Err(e) = remove_dir_all(&pkgbuild.pkgdir) {
            eprintln!("Failed to remove existing pkgdir: {}", e);
            return Err(())
        }
    }
    if let Err(e) = rename(&temp_pkgdir, &pkgbuild.pkgdir) {
        eprintln!(
            "Failed to rename temp pkgdir '{}' to persistent pkgdir '{}': {}",
            temp_pkgdir.display(), pkgbuild.pkgdir.display(), e);
        return Err(())
    }
    let mut rel = PathBuf::from("..");
    rel.push(&pkgbuild.pkgid);
    let updated = PathBuf::from("pkgs/updated");
    for entry in
        pkgbuild.pkgdir.read_dir().expect("Failed to read dir")
    {
        if let Ok(entry) = entry {
            let original = rel.join(entry.file_name());
            let link = updated.join(entry.file_name());
            let _ = symlink(original, link);
        }
    }
    println!("Finished building '{}'", &pkgbuild.pkgid);
    Ok(())
}

fn build(pkgbuild: &PKGBUILD, nonet: bool) -> Result<(), ()> {
    let temp_pkgdir = prepare_temp_pkgdir(pkgbuild)?;
    let mut command = prepare_build_command(
                                pkgbuild, nonet, &temp_pkgdir)?;
    build_try(pkgbuild, &mut command, &temp_pkgdir)?;
    build_finish(pkgbuild, &temp_pkgdir)
}

fn build_any_needed(pkgbuilds: &Vec<PKGBUILD>, nonet: bool) -> Result<(), ()>{
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
    let mut bad = false;
    let mut threads = vec![];
    for pkgbuild in pkgbuilds.iter() {
        if ! pkgbuild.extract {
            continue
        }
        let pkgbuild = pkgbuild.clone();
        if let Err(_) = wait_if_too_busy(
            &mut threads, 5, "building packages") 
        {
            bad = true;
        }
        threads.push(thread::spawn(move || build(&pkgbuild, nonet)));
    }
    if let Err(_) = threading::wait_remaining(
        threads, "building packages") 
    {
        bad = true;
    }
    let thread_cleaner =
        thread::spawn(|| remove_dir_recursively("build"));
    let rel = PathBuf::from("..");
    let latest = PathBuf::from("pkgs/latest");
    for pkgbuild in pkgbuilds.iter() {
        if ! pkgbuild.pkgdir.exists() {
            continue;
        }
        let dirent = match pkgbuild.pkgdir.read_dir() {
            Ok(dirent) => dirent,
            Err(e) => {
                eprintln!("Failed to read dir '{}': {}", 
                    pkgbuild.pkgdir.display(), e);
                continue
            },
        };
        let rel = rel.join(&pkgbuild.pkgid);
        for entry in dirent {
            if let Ok(entry) = entry {
                let original = rel.join(entry.file_name());
                let link = latest.join(entry.file_name());
                println!("Linking '{}' => '{}'", 
                        link.display(), original.display());
                if let Err(e) = symlink(original, link) {
                    eprintln!("Failed to link: {}", e);
                }
            }
        }
    }
    let _ = thread_cleaner.join().expect("Failed to join cleaner thread");
    if bad { Err(()) } else { Ok(()) }
}

fn clean_pkgdir(pkgbuilds: &Vec<PKGBUILD>) {
    let mut used: Vec<String> = pkgbuilds.iter().map(
        |pkgbuild| pkgbuild.pkgid.clone()).collect();
    used.push(String::from("updated"));
    used.push(String::from("latest"));
    used.sort_unstable();
    source::remove_unused("pkgs", &used);
}

pub(crate) fn work<P: AsRef<Path>>(
    pkgbuilds_yaml: P,
    proxy: Option<&str>,
    holdpkg: bool,
    holdgit: bool,
    skipint: bool,
    nobuild: bool,
    noclean: bool,
    nonet: bool,
    gmr: Option<&str>,
) -> Result<(), ()>
{
    let gmr = match gmr {
        Some(gmr) => Some(git::Gmr::init(gmr)),
        None => None,
    };
    let mut pkgbuilds =
        match get_pkgbuilds(
            &pkgbuilds_yaml, holdpkg, noclean, proxy) {
        Some(pkgbuilds) => pkgbuilds,
        None => {
            eprintln!("Failed to get PKGBUILDs");
            return Err(())
        },
    };
    let pkgbuilds_dir =
        tempdir().expect("Failed to create temp dir to dump PKGBUILDs");
    prepare_sources(pkgbuilds_dir, &mut pkgbuilds, 
                    holdgit, skipint, noclean, proxy, gmr.as_ref())?;
    if nobuild {
        return Ok(());
    }
    build_any_needed(&pkgbuilds, nonet)?;
    if noclean {
        return Ok(());
    }
    clean_pkgdir(&pkgbuilds);
    Ok(())
}