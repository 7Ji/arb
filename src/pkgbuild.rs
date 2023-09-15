use std::{path::{PathBuf, Path}, collections::{BTreeMap, HashMap}, thread::{self, sleep, JoinHandle}, io::Write, process::Command, fs::{DirBuilder, remove_dir_all, create_dir_all, rename}, time::Duration, os::unix::fs::symlink, env};
use git2::{Repository, Oid};
use url::Url;
use xxhash_rust::xxh3::xxh3_64;
use crate::{git, source, threading::{self, wait_if_too_busy}};

#[derive(Clone)]
enum Pkgver {
    Plain,
    Func { pkgver: String },
}

#[derive(Clone)]
pub(crate) struct PKGBUILD {
    name: String,
    url: String,
    hash_domain: u64,
    build: PathBuf,
    git: PathBuf,
    pkgid: String,
    pkgdir: PathBuf,
    commit: git2::Oid,
    pkgver: Pkgver,
    extract: bool,
    sources: Vec<source::Source>,
}

struct Repo {
    path: PathBuf,
    url: String,
}

fn read_pkgbuilds_yaml<P>(yaml: P) -> Vec<PKGBUILD>
where 
    P: AsRef<Path>
{
    let f = std::fs::File::open(yaml)
            .expect("Failed to open pkgbuilds YAML config");
    let config: BTreeMap<String, String> = 
        serde_yaml::from_reader(f)
            .expect("Failed to parse into config");
    config.iter().map(|(name, url)| {
        let url_p = Url::parse(url).expect("Invalid URL");
        let hash_domain = match url_p.domain() {
            Some(domain) => xxh3_64(domain.as_bytes()),
            None => 0,
        };
        let mut build = PathBuf::from("build");
        build.push(name);
        let git = PathBuf::from(format!("sources/PKGBUILDs/{}", name));
        PKGBUILD {
            name: name.clone(),
            url: url.clone(),
            hash_domain,
            build,
            git,
            pkgid: String::new(),
            pkgdir: PathBuf::from("pkgs"),
            commit: Oid::zero(),
            pkgver: Pkgver::Plain,
            extract: false,
            sources: vec![],
        }
    }).collect()
}

fn sync_pkgbuilds(pkgbuilds: &Vec<PKGBUILD>, proxy: Option<&str>) {
    let mut map: HashMap<u64, Vec<Repo>> = HashMap::new();
    for pkgbuild in pkgbuilds.iter() {
        if ! map.contains_key(&pkgbuild.hash_domain) {
            println!("New domain found from PKGBUILD URL: {}", pkgbuild.url);
            map.insert(pkgbuild.hash_domain, vec![]);
        }
        let vec = map
            .get_mut(&pkgbuild.hash_domain)
            .expect("Failed to get vec");
        vec.push(Repo { path: pkgbuild.git.clone(), url: pkgbuild.url.clone() });
    }
    println!("Syncing PKGBUILDs with {} threads", map.len());
    const REFSPECS: &[&str] = &["+refs/heads/master:refs/heads/master"];
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads =  Vec::new();
    for repos in map.into_values() {
        let proxy_string_thread = proxy_string.clone();
        threads.push(thread::spawn(move || {
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            for repo in repos {
                git::sync_repo(&repo.path, &repo.url, proxy, REFSPECS);
            }
        }));
    }
    for thread in threads.into_iter() {
        thread.join().expect("Failed to join");
    }
}

fn get_pkgbuild_blob(repo: &Repository) -> Option<git2::Blob> {
    git::get_branch_entry_blob(repo, "master", "PKGBUILD")
}

fn healthy_pkgbuild(pkgbuild: &mut PKGBUILD, set_commit: bool) -> bool {
    let repo = 
        match git::open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url) {
            Some(repo) => repo,
            None => {
                eprintln!("Failed to open or init bare repo {}", pkgbuild.git.display());
                return false
            }
        };
    if set_commit {
        match git::get_branch_commit_id(&repo, "master") {
            Some(id) => pkgbuild.commit = id,
            None => {
                eprintln!("Failed to set commit id for pkgbuild {}", pkgbuild.name);
                return false
            },
        }
    }
    println!("PKGBUILD '{}' at commit '{}'", pkgbuild.name, pkgbuild.commit);
    match get_pkgbuild_blob(&repo) {
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
            git::open_or_init_bare_repo(&pkgbuild.git, &pkgbuild.url)
            .expect("Failed to open repo");
        let blob = 
            get_pkgbuild_blob(&repo)
            .expect("Failed to get PKGBUILD blob");
        let mut file = 
            std::fs::File::create(path)
            .expect("Failed to create file");
        file.write_all(blob.content()).expect("Failed to write");
    }
}

fn get_dep<P: AsRef<Path>> (pkgbuild: P) -> Vec<String> {
    const SCRIPT: &str = include_str!("scripts/get_depends.bash");
    let output = Command::new("/bin/bash")
        .env_clear()
        .arg("-ec")
        .arg(SCRIPT)
        .arg("Depends reader")
        .arg(pkgbuild.as_ref())
        .output()
        .expect("Failed to run depends reader");
    let mut deps = vec![];
    for line in output.stdout.split(|byte| byte == &b'\n') {
        if line.len() == 0 {
            continue;
        }
        deps.push(String::from_utf8_lossy(line).into_owned());
    }
    deps
}

fn ensure_deps<P: AsRef<Path>> (dir: P, pkgbuilds: &mut Vec<PKGBUILD>) {
    let mut threads: Vec<JoinHandle<Vec<String>>> = vec![];
    let mut deps = vec![];
    for pkgbuild in pkgbuilds.iter() {
        let pkgbuild_file = dir.as_ref().join(&pkgbuild.name);
        threading::wait_if_too_busy_with_callback(&mut threads, 30, |mut other| {
            deps.append(&mut other);
        });
        threads.push(thread::spawn(move || get_dep(&pkgbuild_file)));
    }
    for thread in threads {
        let mut other = thread.join().expect("Failed to join finished thread");
        deps.append(&mut other);
    }
    if deps.len() == 0 {
        return
    }
    deps.sort();
    deps.dedup();
    println!("Ensuring {} deps: {:?}", deps.len(), deps);
    let output = Command::new("/usr/bin/pacman")
        .env_clear()
        .arg("-T")
        .args(&deps)
        .output()
        .expect("Failed to run pacman to get missing deps");
    match output.status.code() {
        Some(code) => match code {
            0 => return,
            127 => (),
            _ => {
                eprintln!("Pacman returned unexpected {} which marks fatal error", code);
                panic!("Pacman fatal error");
            }
        },
        None => panic!("Failed to get return code from pacman"),
    }
    deps.clear();
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.len() == 0 {
            continue;
        }
        deps.push(String::from_utf8_lossy(line).into_owned());
    }
    if deps.len() == 0 {
        return;
    }

    println!("Installing {} missing deps: {:?}", deps.len(), deps);
    let mut child = Command::new("/usr/bin/sudo")
        .env_clear()
        .arg("/usr/bin/pacman")
        .arg("-S")
        .arg("--noconfirm")
        .args(&deps)
        .spawn()
        .expect("Failed to run sudo pacman to install missing deps");
    let exit_status = child.wait().expect("Failed to wait for child sudo pacman process");
    if let Some(code) = exit_status.code() {
        if code == 0 {
            return
        }
        println!("Failed to run sudo pacman, return: {}", code);
    }
    panic!("Sudo pacman process not successful");
}

fn get_all_sources<P: AsRef<Path>> (dir: P, pkgbuilds: &mut Vec<PKGBUILD>) 
    -> (Vec<source::Source>, Vec<source::Source>, Vec<source::Source>) {
    let mut sources_non_unique = vec![];
    for pkgbuild in pkgbuilds.iter_mut() {
        pkgbuild.sources = source::get_sources::<P>(&dir.as_ref().join(&pkgbuild.name))
    }
    for pkgbuild in pkgbuilds.iter() {
        for source in pkgbuild.sources.iter() {
            sources_non_unique.push(source);
        }
    }
    source::unique_sources(&sources_non_unique)
}

pub(crate) fn get_pkgbuilds<P>(config: P, hold: bool, proxy: Option<&str>) -> Vec<PKGBUILD>
where 
    P:AsRef<Path>
{
    let mut pkgbuilds = read_pkgbuilds_yaml(config);
    let update_pkg = if hold {
        if healthy_pkgbuilds(&mut pkgbuilds, true) {
            println!("Holdpkg set and all PKGBUILDs healthy, no need to update");
            false
        } else {
            eprintln!("Warning: holdpkg set, but unhealthy PKGBUILDs found, still need to update");
            true
        }
    } else {
        true
    };
    if update_pkg {
        sync_pkgbuilds(&pkgbuilds, proxy);
        if ! healthy_pkgbuilds(&mut pkgbuilds, true) {
            panic!("Updating broke some of our PKGBUILDs");
        }
    }
    pkgbuilds
}

fn extract_source<P: AsRef<Path>>(dir: P, repo: P, sources: &Vec<source::Source>) {
    create_dir_all(&dir).expect("Failed to create dir");
    git::checkout_branch_from_repo(&dir, &repo, "master");
    source::extract(&dir, sources);
    const SCRIPT: &str = include_str!("scripts/extract_sources.bash");
    Command::new("/bin/bash")
        .env_clear()
        .arg("-ec")
        .arg(SCRIPT)
        .arg("Source extractor")
        .arg(dir.as_ref().canonicalize().expect("Failed to canonicalize dir"))
        .spawn()
        .expect("Failed to run script")
        .wait()
        .expect("Failed to wait for spawned script");
}

fn extract_source_and_get_pkgver<P: AsRef<Path>>(dir: P, repo: P, sources: &Vec<source::Source>) -> String {
    extract_source(&dir, &repo, sources);
    let output = Command::new("/bin/bash")
        .env_clear()
        .arg("-ec")
        .arg("cd $1; source ../PKGBUILD; pkgver")
        .arg("Pkgver runner")
        .arg(dir.as_ref().join("src").canonicalize().expect("Failed to canonicalize dir"))
        .output()
        .expect("Failed to run script");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn fill_all_pkgvers<P: AsRef<Path>>(dir: P, pkgbuilds: &mut Vec<PKGBUILD>) {
    let _ = remove_dir_all("build");
    let dir = dir.as_ref();
    for pkgbuild in pkgbuilds.iter_mut() {
        let output = Command::new("/bin/bash")
            .env_clear()
            .arg("-c")
            .arg(". \"$1\"; type -t pkgver")
            .arg("Type Identifier")
            .arg(dir.join(&pkgbuild.name))
            .output()
            .expect("Failed to run script");
        pkgbuild.extract = match output.stdout.as_slice() {
            b"function\n" => true,
            _ => false,
        }
    }
    let mut dir_builder = DirBuilder::new();
    dir_builder.recursive(true);
    struct PkgbuildThread<'a> {
        pkgbuild: &'a mut PKGBUILD,
        thread: JoinHandle<String>
    }
    let mut pkgbuild_threads: Vec<PkgbuildThread> = vec![];
    for pkgbuild in pkgbuilds.iter_mut().filter(|pkgbuild| pkgbuild.extract) {
        let dir = pkgbuild.build.clone();
        let repo = pkgbuild.git.clone();
        let sources = pkgbuild.sources.clone();
        if pkgbuild_threads.len() > 20 {
            let mut thread_id_finished = None;
            loop {
                for (thread_id, pkgbuild_thread) in pkgbuild_threads.iter().enumerate() {
                    if pkgbuild_thread.thread.is_finished() {
                        thread_id_finished = Some(thread_id);
                        break;
                    }
                }
                if let None = thread_id_finished {
                    sleep(Duration::from_millis(10));
                } else {
                    break
                }
            }
            if let Some(thread_id_finished) = thread_id_finished {
                let pkgbuild_thread = pkgbuild_threads.swap_remove(thread_id_finished);
                let pkgver = pkgbuild_thread.thread.join().expect("Failed to join finished thread");
                pkgbuild_thread.pkgbuild.pkgver = Pkgver::Func { pkgver };
            } else {
                panic!("Failed to get finished thread ID")
            }
        }
        pkgbuild_threads.push(PkgbuildThread { pkgbuild, thread: thread::spawn(move || extract_source_and_get_pkgver(dir, repo, &sources))});
    }
    for pkgbuild_thread in pkgbuild_threads {
        let pkgver = pkgbuild_thread.thread.join().expect("Failed to join finished thread");
        pkgbuild_thread.pkgbuild.pkgver = Pkgver::Func { pkgver };
    }
}

fn fill_all_pkgdirs(pkgbuilds: &mut Vec<PKGBUILD>) {
    for pkgbuild in pkgbuilds.iter_mut() {
        let mut pkgid = format!("{}-{}", pkgbuild.name, pkgbuild.commit);
        if let Pkgver::Func { pkgver } = &pkgbuild.pkgver {
            pkgid.push('-');
            pkgid.push_str(&pkgver);
        }
        pkgbuild.pkgdir.push(&pkgid);
        pkgbuild.pkgid = pkgid;
        println!("Pkgdir for '{}': '{}'", pkgbuild.name, pkgbuild.pkgdir.display());
    }
}

fn extract_if_need_build(pkgbuilds: &mut Vec<PKGBUILD>) {
    let mut threads = vec![];
    for pkgbuild in pkgbuilds.iter_mut() {
        let mut built = false;
        if let Ok(mut dir) = pkgbuild.pkgdir.read_dir() {
            if let Some(_) = dir.next() {
                built = true;
            }
        }
        if built { // Does not need build
            println!("'{}' already built, no need to build", pkgbuild.pkgdir.display());
            if pkgbuild.extract {
                let dir = pkgbuild.build.clone();
                wait_if_too_busy(&mut threads, 20);
                threads.push(thread::spawn(|| remove_dir_all(dir).expect("Failed to remove dir")));
                pkgbuild.extract = false;
            }
        } else {
            if ! pkgbuild.extract {
                let dir = pkgbuild.build.clone();
                let repo = pkgbuild.git.clone();
                let sources = pkgbuild.sources.clone();
                wait_if_too_busy(&mut threads, 20);
                threads.push(thread::spawn(move || extract_source(dir, repo, &sources)));
                pkgbuild.extract = true;
            }
        }
    }
    for thread in threads {
        thread.join().expect("Failed to join finished thread");
    }
}

pub(crate) fn prepare_sources<P: AsRef<Path>>(dir: P, pkgbuilds: &mut Vec<PKGBUILD>, holdgit: bool, skipint: bool, proxy: Option<&str>) {
    let thread_cleaner = thread::spawn(|| remove_dir_all("build"));
    dump_pkgbuilds(&dir, pkgbuilds);
    ensure_deps(&dir, pkgbuilds);
    let (netfile_sources, git_sources, _) 
        = get_all_sources(&dir, pkgbuilds);
    source::cache_sources_mt(&netfile_sources, &git_sources, holdgit, skipint, proxy);
    let _ = thread_cleaner.join().expect("Failed to join cleaner thread");
    fill_all_pkgvers(dir, pkgbuilds);
    fill_all_pkgdirs(pkgbuilds);
    extract_if_need_build(pkgbuilds);
}

fn build(pkgbuild: &PKGBUILD) {
    let mut temp_name = pkgbuild.pkgdir.file_name().expect("Failed to get file name").to_os_string();
    temp_name.push(".temp");
    let temp_pkgdir = pkgbuild.pkgdir.with_file_name(temp_name);
    let _ = create_dir_all(&temp_pkgdir);
    Command::new("/usr/bin/makepkg")
        .current_dir(&pkgbuild.build)
        .env_clear()
        .env("PATH", env::var_os("PATH").expect("Failed to get PATH env"))
        .env("HOME", env::var_os("HOME").expect("Failed to get HOME env"))
        .env("PKGDEST", &temp_pkgdir.canonicalize().expect("Failed to get absolute path of pkgdir"))
        .env("PKGEXT", ".pkg.tar")
        .arg("--holdver")
        .arg("--noextract")
        .arg("--ignorearch")
        .spawn()
        .expect("Failed to spawn makepkg")
        .wait()
        .expect("Failed to wait for makepkg");
    let build = pkgbuild.build.clone();
    let thread_cleaner = thread::spawn(|| remove_dir_all(build));
    let _ = remove_dir_all(&pkgbuild.pkgdir);
    rename(&temp_pkgdir, &pkgbuild.pkgdir).expect("Failed to move result pkgdir");
    let mut rel = PathBuf::from("..");
    rel.push(&pkgbuild.pkgid);
    let updated = PathBuf::from("pkgs/updated");
    for entry in pkgbuild.pkgdir.read_dir().expect("Failed to read dir") {
        if let Ok(entry) = entry {
            let original = rel.join(entry.file_name());
            let link = updated.join(entry.file_name());
            symlink(original, link).expect("Failed to symlink");
        }
    }
    let _ = thread_cleaner.join().expect("Failed to join cleaner thread");
}

pub(crate) fn build_any_needed(pkgbuilds: &Vec<PKGBUILD>) {
    let _ = remove_dir_all("pkgs/updated");
    let _ = remove_dir_all("pkgs/latest");
    let _ = create_dir_all("pkgs/updated");
    let _ = create_dir_all("pkgs/latest");
    let mut threads = vec![];
    for pkgbuild in pkgbuilds.iter() {
        if ! pkgbuild.extract {
            continue
        }
        let pkgbuild = pkgbuild.clone();
        wait_if_too_busy(&mut threads, 3);
        threads.push(thread::spawn(move || build(&pkgbuild)));
    }
    for thread in threads {
        thread.join().expect("Failed to join finished builder thread");
    }
    let thread_cleaner = thread::spawn(|| remove_dir_all("build"));
    let rel = PathBuf::from("..");
    let latest = PathBuf::from("pkgs/latest");
    for pkgbuild in pkgbuilds.iter() {
        let rel = rel.join(&pkgbuild.pkgid);
        for entry in pkgbuild.pkgdir.read_dir().expect("Failed to read dir") {
            if let Ok(entry) = entry {
                let original = rel.join(entry.file_name());
                let link = latest.join(entry.file_name());
                let _ = symlink(original, link);
            }
        }
    }
    let _ = thread_cleaner.join().expect("Failed to join cleaner thread");
}