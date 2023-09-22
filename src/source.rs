use crate::{
        cksums,
        download,
        git::{
            self,
            ToReposMap,
        },
        threading,
    };
use hex::FromHex;
use std::{
        collections::HashMap,
        fs::{
            DirBuilder,
            read_dir,
            remove_dir_all,
            remove_file,
        },
        os::unix::fs::symlink,
        path::{
            Path,
            PathBuf,
        },
        process::Command,
        str::FromStr,
        thread::{
            self,
            JoinHandle,
        }
    };
use xxhash_rust::xxh3::xxh3_64;


#[derive(Debug, Clone)]
enum NetfileProtocol {
    File,
    Ftp,
    Http,
    Https,
    Rsync,
    Scp,
}

#[derive(Debug, Clone)]
enum VcsProtocol {
    Bzr,
    Fossil,
    Git,
    Hg,
    Svn,
}

#[derive(Debug, Clone)]
enum Protocol {
    Netfile {
        protocol: NetfileProtocol
    },
    Vcs {
        protocol: VcsProtocol
    },
    Local
}

impl Protocol {
    fn _from_string(value: &str) -> Protocol {
        match value {
            "file" => Protocol::Netfile { protocol: NetfileProtocol::File },
            "ftp" => Protocol::Netfile { protocol: NetfileProtocol::Ftp },
            "http" => Protocol::Netfile { protocol: NetfileProtocol::Http },
            "https" => Protocol::Netfile { protocol: NetfileProtocol::Https },
            "rsync" => Protocol::Netfile { protocol: NetfileProtocol::Rsync },
            "scp" => Protocol::Netfile { protocol: NetfileProtocol::Scp },
            "bzr" => Protocol::Vcs { protocol: VcsProtocol::Bzr },
            "fossil" => Protocol::Vcs { protocol: VcsProtocol::Fossil },
            "git" => Protocol::Vcs { protocol: VcsProtocol::Git },
            "hg" => Protocol::Vcs { protocol: VcsProtocol::Hg },
            "svn" => Protocol::Vcs { protocol: VcsProtocol::Svn },
            "local" => Protocol::Local,
            &_ => {
                eprintln!("Unknown protocol {}", value);
                panic!("Unknown protocol");
            },
        }
    }
    fn from_raw_string(value: &[u8]) -> Protocol {
        match value {
            b"file" => Protocol::Netfile { protocol: NetfileProtocol::File },
            b"ftp" => Protocol::Netfile { protocol: NetfileProtocol::Ftp },
            b"http" => Protocol::Netfile { protocol: NetfileProtocol::Http },
            b"https" => Protocol::Netfile { protocol: NetfileProtocol::Https },
            b"rsync" => Protocol::Netfile { protocol: NetfileProtocol::Rsync },
            b"scp" => Protocol::Netfile { protocol: NetfileProtocol::Scp },
            b"bzr" => Protocol::Vcs { protocol: VcsProtocol::Bzr },
            b"fossil" => Protocol::Vcs { protocol: VcsProtocol::Fossil },
            b"git" => Protocol::Vcs { protocol: VcsProtocol::Git },
            b"hg" => Protocol::Vcs { protocol: VcsProtocol::Hg },
            b"svn" => Protocol::Vcs { protocol: VcsProtocol::Svn },
            b"local" => Protocol::Local,
            &_ => {
                panic!("Unknown protocol");
            },
        }
    }
}
#[derive(Clone)]
pub(crate) struct Source {
    name: String,
    protocol: Protocol,
    url: String,
    hash_url: u64,
    ck: Option<u32>,     // 32-bit CRC
    md5: Option<[u8; 16]>,   // 128-bit MD5
    sha1: Option<[u8; 20]>,  // 160-bit SHA-1
    sha224: Option<[u8; 28]>,// 224-bit SHA-2
    sha256: Option<[u8; 32]>,// 256-bit SHA-2
    sha384: Option<[u8; 48]>,// 384-bit SHA-2
    sha512: Option<[u8; 64]>,// 512-bit SHA-2
    b2: Option<[u8; 64]>,    // 512-bit Blake-2B
}

pub(crate) trait MapByDomain {
    fn url(&self) -> &str;
    fn map_by_domain(sources: &Vec<Self>) -> HashMap<u64, Vec<Self>>
    where
        Self: Clone + Sized
    {
        let mut map = HashMap::new();
        for source in sources.iter() {
            let url =
                url::Url::from_str(source.url())
                .expect("Failed to parse URL");
            let domain = xxh3_64(
                url.domain().expect("Failed to get domain")
                .as_bytes());
            if ! map.contains_key(&domain) {
                map.insert(domain, vec![]);
            }
            let vec = map
                .get_mut(&domain)
                .expect("Failed to get vec");
            vec.push(source.clone());
        }
        map
    }
}

impl MapByDomain for Source {
    fn url(&self) -> &str {
        self.url.as_str()
    }
}

impl git::ToReposMap for Source {
    fn url(&self) -> &str {
        self.url.as_str()
    }

    fn path(&self) -> Option<&Path> {
        None
    }
}


fn push_source(
    sources: &mut Vec<Source>,
    name: Option<String>,
    protocol: Option<Protocol>,
    url: Option<String>,
    hash_url: u64,
    ck: Option<u32>,     // 32-bit CRC
    md5: Option<[u8; 16]>,   // 128-bit MD5
    sha1: Option<[u8; 20]>,  // 160-bit SHA-1
    sha224: Option<[u8; 28]>,// 224-bit SHA-2
    sha256: Option<[u8; 32]>,// 256-bit SHA-2
    sha384: Option<[u8; 48]>,// 384-bit SHA-2
    sha512: Option<[u8; 64]>,// 512-bit SHA-2
    b2: Option<[u8; 64]>,    // 512-bit Blake-2B
) {
    if let None = ck {
    if let None = md5 {
    if let None = sha1 {
    if let None = sha224 {
    if let None = sha256 {
    if let None = sha384 {
    if let None = sha512 {
    if let None = b2 {
    if let Some(protocol) = &protocol {
    if let Protocol::Netfile { protocol: _ } = protocol {
        return
    }}}}}}}}}}
    if let Some(name) = name {
        if let Some(protocol) = protocol {
            if let Some(url) = url {
                sources.push(Source{
                    name,
                    protocol,
                    url,
                    hash_url,
                    ck,
                    md5,
                    sha1,
                    sha224,
                    sha256,
                    sha384,
                    sha512,
                    b2,
                });
                return
            }
        }
    };
    panic!("Unfinished source definition")
}

pub(crate) fn get_sources<P> (pkgbuild: &Path) -> Vec<Source>
where
    P: AsRef<Path>
{
    const SCRIPT: &str = include_str!("scripts/get_sources.bash");
    let output = Command::new("/bin/bash")
        .env_clear()
        .arg("-ec")
        .arg(SCRIPT)
        .arg("Source reader")
        .arg(pkgbuild)
        .output()
        .expect("Failed to run script");
    let mut name = None;
    let mut protocol = None;
    let mut url = None;
    let mut hash_url = 0;
    let mut ck = None;
    let mut md5 = None;
    let mut sha1 = None;
    let mut sha224 = None;
    let mut sha256 = None;
    let mut sha384 = None;
    let mut sha512 = None;
    let mut b2 = None;
    let mut sources = vec![];
    let mut started = false;
    for line in  output.stdout.split(|byte| byte == &b'\n') {
        if line.len() == 0 {
            continue;
        }
        if line == b"[source]" {
            if started {
                push_source(&mut sources,
                    name, protocol, url, hash_url,
                    ck, md5, sha1,
                    sha224, sha256, sha384, sha512,
                    b2);
                name = None;
                protocol = None;
                url = None;
                hash_url = 0;
                ck = None;
                md5 = None;
                sha1 = None;
                sha224 = None;
                sha256 = None;
                sha384 = None;
                sha512 = None;
                b2 = None;
            } else {
                started = true;
            }
            continue;
        }
        let mut it =
            line.splitn(2, |byte| byte == &b':');
        let key = it.next().expect("Failed to get key");
        let value = it.next().expect("Failed to get value");
        match key {
            b"name" => {
                name = Some(String::from_utf8_lossy(value).into_owned());
            }
            b"protocol" => {
                protocol = Some(Protocol::from_raw_string(value));
            }
            b"url" => {
                url = Some(String::from_utf8_lossy(value).into_owned());
                hash_url = xxh3_64(value);
            }
            b"cksum" => {
                ck = Some(
                        String::from_utf8_lossy(value)
                        .parse()
                        .expect("Failed to parse 32-bit CRC"));
            }
            b"md5sum" => {
                md5 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 128-bit MD5 sum"));
            }
            b"sha1sum" => {
                sha1 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 160-bit SHA-1 sum"));
            }
            b"sha224sum" => {
                sha224 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 224-bit SHA-2 sum"));
            }
            b"sha256sum" => {
                sha256 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 256-bit SHA-2 sum"));
            }
            b"sha384sum" => {
                sha384 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 384-bit SHA-2 sum"));
            }
            b"sha512sum" => {
                sha512 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 512-bit SHA-2 sum"));
            }
            b"b2sum" => {
                b2 = Some(
                        FromHex::from_hex(value)
                        .expect("Failed to parse 512-bit Blake-2B sum"));
            }
            &_ => {
                panic!("Unexpected line");
            }
        }
    }
    push_source(&mut sources,
        name, protocol, url, hash_url,
        ck, md5, sha1,
        sha224, sha256, sha384, sha512,
        b2);
    sources
}

fn push_netfile_sources(netfile_sources: &mut Vec<Source>, source: &Source) {
    let mut existing = None;
    for netfile_source in netfile_sources.iter_mut() {
        if cksums::optional_equal(
                &netfile_source.ck, &source.ck) ||
           cksums::optional_equal(
                &netfile_source.md5, &source.md5) ||
           cksums::optional_equal(
                &netfile_source.sha1, &source.sha1) ||
           cksums::optional_equal(
                &netfile_source.sha224, &source.sha224) ||
           cksums::optional_equal(
                &netfile_source.sha256, &source.sha256) ||
           cksums::optional_equal(
                &netfile_source.sha384, &source.sha384) ||
           cksums::optional_equal(
                &netfile_source.sha512, &source.sha512) ||
           cksums::optional_equal(&netfile_source.b2, &source.b2) {
            existing = Some(netfile_source);
            break;
        }
    }
    let netfile_source = match existing {
        Some(netfile_source) => netfile_source,
        None => {
            netfile_sources.push(source.clone());
            netfile_sources.last_mut()
                .expect("Failed to get unique source we just added")
        },
    };
    cksums::optional_update(
        &mut netfile_source.ck, &source.ck);
    cksums::optional_update(
        &mut netfile_source.md5, &source.md5);
    cksums::optional_update(
        &mut netfile_source.sha1, &source.sha1);
    cksums::optional_update(
        &mut netfile_source.sha224, &source.sha224);
    cksums::optional_update(
        &mut netfile_source.sha256, &source.sha256);
    cksums::optional_update(
        &mut netfile_source.sha384, &source.sha384);
    cksums::optional_update(
        &mut netfile_source.sha512, &source.sha512);
    cksums::optional_update(
        &mut netfile_source.b2, &source.b2);
}

fn push_git_sources(git_sources: &mut Vec<Source>, source: &Source) {
    for git_source in git_sources.iter() {
        if git_source.hash_url == source.hash_url {
            return
        }
    }
    git_sources.push(source.clone())
}

pub(crate) fn unique_sources(sources: &Vec<&Source>)
    -> (Vec<Source>, Vec<Source>, Vec<Source>)
{
    let mut local_sources: Vec<Source> = vec![];
    let mut git_sources: Vec<Source> = vec![];
    let mut netfile_sources: Vec<Source> = vec![];
    for source in sources.iter() {
        match &source.protocol {
            Protocol::Netfile { protocol: _ } =>
                push_netfile_sources(&mut netfile_sources, source),
            Protocol::Vcs { protocol } => {
                match protocol {  // Ignore VCS sources we do not support
                    VcsProtocol::Bzr => (),
                    VcsProtocol::Fossil => (),
                    VcsProtocol::Git =>
                        push_git_sources(&mut git_sources, source),
                    VcsProtocol::Hg => (),
                    VcsProtocol::Svn => (),
                }
            },
            Protocol::Local => local_sources.push(source.to_owned().to_owned())
        }
    }
    (netfile_sources, git_sources, local_sources)
}

fn _print_source(source: &Source) {
    println!("Source '{}' from '{}' protocol '{:?}'",
        source.name, source.url, source.protocol);
    if let Some(ck) = source.ck {
        println!("=> CKSUM: {:x}", ck);
    }
    if let Some(md5) = source.md5 {
        println!("=> md5sum: {}", cksums::string_from(&md5));
    }
    if let Some(sha1) = source.sha1 {
        println!("=> sha1sum: {}", cksums::string_from(&sha1));
    }
    if let Some(sha224) = source.sha224 {
        println!("=> sha224sum: {}", cksums::string_from(&sha224));
    }
    if let Some(sha256) = source.sha256 {
        println!("=> sha256sum: {}", cksums::string_from(&sha256));
    }
    if let Some(sha384) = source.sha384 {
        println!("=> sha384sum: {}", cksums::string_from(&sha384));
    }
    if let Some(sha512) = source.sha512 {
        println!("=> sha512sum: {}", cksums::string_from(&sha512));
    }
    if let Some(b2) = source.b2 {
        println!("=> b2sum: {}", cksums::string_from(&b2));
    }
}

fn get_integ_files(source: &Source) -> Vec<cksums::IntegFile> {
    let mut integ_files = vec![];
    if let Some(ck) = source.ck {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-ck", cksums::Integ::CK { ck }))
    }
    if let Some(md5) = source.md5 {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-md5", cksums::Integ::MD5 { md5 }))
    }
    if let Some(sha1) = source.sha1 {
        integ_files.push(cksums::IntegFile::from_integ
            ("sources/file-sha1", cksums::Integ::SHA1 { sha1 }))
    }
    if let Some(sha224) = source.sha224 {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-sha224", cksums::Integ::SHA224 { sha224 }))
    }
    if let Some(sha256) = source.sha256 {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-sha256", cksums::Integ::SHA256 { sha256 } ))
    }
    if let Some(sha384) = source.sha384 {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-sha384", cksums::Integ::SHA384 { sha384 } ))
    }
    if let Some(sha512) = source.sha512 {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-sha512", cksums::Integ::SHA512 { sha512 }))
    }
    if let Some(b2) = source.b2 {
        integ_files.push(cksums::IntegFile::from_integ(
            "sources/file-b2", cksums::Integ::B2 { b2 } ))
    }
    integ_files
}

fn download_netfile_source(
    netfile_source: &Source,
    integ_file: &cksums::IntegFile,
    skipint: bool,
    proxy: Option<&str>
) {
    let protocol = match &netfile_source.protocol {
        Protocol::Netfile { protocol } => protocol.clone(),
        Protocol::Vcs { protocol: _ } =>
            panic!("VCS source encountered by netfile cacher"),
        Protocol::Local => panic!("Local source encountered by netfile cacher"),
    };
    let url = netfile_source.url.as_str();
    let path = integ_file.get_path();
    for _ in 0..2 {
        println!("Downloading '{}' to '{}'",
            netfile_source.url, path.display());
        match &protocol {
            NetfileProtocol::File => download::file(url, path),
            NetfileProtocol::Ftp => download::ftp(url, path),
            NetfileProtocol::Http => download::http(url, path, None),
            NetfileProtocol::Https => download::http(url, path, None),
            NetfileProtocol::Rsync => download::rsync(url, path),
            NetfileProtocol::Scp => download::scp(url, path),
        }
        if integ_file.valid(skipint) {
            return
        }
    }
    if let Some(_) = proxy {
        if match &protocol {
            NetfileProtocol::File => false,
            NetfileProtocol::Ftp => false,
            NetfileProtocol::Http => true,
            NetfileProtocol::Https => true,
            NetfileProtocol::Rsync => false,
            NetfileProtocol::Scp => false,
        } {
            println!("Failed to download '{}' to '{}' after 3 tries, use proxy",
                    netfile_source.url, path.display());
            for _  in 0..2 {
                println!("Downloading '{}' to '{}'",
                        netfile_source.url, path.display());
                download::http(url, path, proxy);
                if integ_file.valid(skipint) {
                    return
                }
            }
        }
    }
    panic!("Failed to download netfile source '{}'", netfile_source.url);
}

fn cache_netfile_source(
    netfile_source: &Source,
    integ_files: &Vec<cksums::IntegFile>,
    skipint: bool,
    proxy: Option<&str>
) {
    println!("Caching '{}'", netfile_source.url);
    if integ_files.len() == 0 {
        panic!("No integ files")
    }
    let mut good_files = vec![];
    let mut bad_files = vec![];
    for integ_file in integ_files.iter() {
        println!("'{}' <= '{}'",
            integ_file.get_path().display(),
            netfile_source.url);
        if integ_file.valid(skipint) {
            good_files.push(integ_file);
        } else {
            bad_files.push(integ_file);
        }
    }
    let bad_count = bad_files.len();
    if bad_count > 0 {
        println!("Missing integ files for '{}': {}",
                netfile_source.url, bad_count);
    } else {
        println!("All integ files healthy for '{}'", netfile_source.url);
        return
    }
    while let Some(bad_file) = bad_files.pop() {
        match good_files.last() {
            Some(good_file) =>
                bad_file.clone_file_from(good_file),
            None => download_netfile_source(
                netfile_source, bad_file, skipint, proxy),
        }
        good_files.push(bad_file);
    }
}

fn cache_netfile_sources_for_domain_mt(
    netfile_sources: Vec<Source>, skipint:bool, proxy: Option<&str>
) {
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads: Vec<JoinHandle<()>> = vec![];
    for netfile_source in netfile_sources {
        let integ_files = get_integ_files(&netfile_source);
        let proxy_string_thread = proxy_string.clone();
        threading::wait_if_too_busy(&mut threads, 10,
            "caching network files");
        threads.push(thread::spawn(move ||{
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            cache_netfile_source(&netfile_source, &integ_files, skipint, proxy)
        }));
    }
    threading::wait_remaining(threads, "caching network files");
}

fn ensure_netfile_parents() {
    let mut dir_builder = DirBuilder::new();
    dir_builder.recursive(true);
    for integ in
        ["ck", "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "b2"]
    {
        let folder = format!("sources/file-{}", integ);
        match dir_builder.create(&folder) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to create folder '{}': {}", &folder, e);
                panic!("Failed to ensure netfile parents");
            },
        }
    }
}

fn cache_netfile_sources_mt(
    netfile_sources: HashMap<u64, Vec<Source>>,
    skipint: bool,
    proxy: Option<&str>
) {
    ensure_netfile_parents();
    println!("Caching netfile sources with {} threads", netfile_sources.len());
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let mut threads: Vec<std::thread::JoinHandle<()>> =  Vec::new();
    for netfile_sources in netfile_sources.into_values() {
        let proxy_string_thread = proxy_string.clone();
        threads.push(thread::spawn(move || {
            let proxy = match has_proxy {
                true => Some(proxy_string_thread.as_str()),
                false => None,
            };
            cache_netfile_sources_for_domain_mt(
                netfile_sources, skipint, proxy);
        }));
    }
    for thread in threads {
        thread.join().expect("Failed to join netfile cacher threads");
    }
}

fn cache_git_sources_mt(
    git_sources_map: HashMap<u64, Vec<Source>>,
    holdgit: bool,
    proxy: Option<&str>
) {
    let repos_map =
        Source::to_repos_map(git_sources_map, "sources/git");
    git::Repo::sync_mt(
        repos_map, git::Refspecs::HeadsTags, holdgit, proxy)
}

pub(crate) fn cache_sources_mt(
    netfile_sources: &Vec<Source>,
    git_sources: &Vec<Source>,
    holdgit: bool,
    skipint: bool,
    proxy: Option<&str>
) {
    let netfile_sources_map =
        Source::map_by_domain(netfile_sources);
    let git_sources_map =
        Source::map_by_domain(git_sources);
    let (proxy_string, has_proxy) = match proxy {
        Some(proxy) => (proxy.to_owned(), true),
        None => (String::new(), false),
    };
    let proxy_string_dup = proxy_string.clone();
    let netfile_thread = std::thread::spawn(move || {
        let proxy = match has_proxy {
            true => Some(proxy_string_dup.as_str()),
            false => None,
        };
        cache_netfile_sources_mt(netfile_sources_map, skipint, proxy)
    });
    let git_thread = std::thread::spawn(move || {
        let proxy = match has_proxy {
            true => Some(proxy_string.as_str()),
            false => None,
        };
        cache_git_sources_mt(git_sources_map, holdgit, proxy)
    });
    netfile_thread.join().expect("Failed to join netfile thread");
    git_thread.join().expect("Failed to join git thread");
    println!("Finished multi-threading caching sources");
}

pub(crate) fn extract<P: AsRef<Path>>(dir: P, sources: &Vec<Source>) {
    let rel = PathBuf::from("../..");
    for source in sources.iter() {
        let mut original = None;
        match &source.protocol {
            Protocol::Netfile { protocol: _ } => {
                let integ_files = get_integ_files(source);
                if let Some(integ_file) = integ_files.last() {
                    original = Some(rel.join(integ_file.get_path()));
                }
            },
            Protocol::Vcs { protocol } =>
                if let VcsProtocol::Git = protocol {
                    original = Some(rel
                        .join(format!("sources/git/{:016x}",
                                xxh3_64(source.url.as_bytes()))));
                },
            Protocol::Local => (),
        }
        if let Some(original) = original {
            symlink(original,
                dir.as_ref().join(&source.name))
                .expect("Failed to symlink")
        }
    }
}

// Used must be already sorted
pub(crate) fn remove_unused<P: AsRef<Path>>(dir: P, used: &Vec<String>) {
    let readdir = match read_dir(dir) {
        Ok(readdir) => readdir,
        Err(_) => return,
    };
    for entry in readdir {
        match entry {
            Ok(entry) => {
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                let name =
                    entry.file_name().to_string_lossy().into_owned();
                match used.binary_search(&name) {
                    Ok(_) => continue,
                    Err(_) => (),
                }
                if metadata.is_dir() {
                    println!("Removing '{}' not used any more",
                        entry.path().display());
                    let _ = remove_dir_all(entry.path());
                }
                if metadata.is_file() || metadata.is_symlink() {
                    println!("Removing '{}' not used any more",
                        entry.path().display());
                    let _ = remove_file(entry.path());
                }
            },
            Err(_) => return,
        }
    }
}

fn clean_netfile_sources(sources: &Vec<Source>) -> Vec<JoinHandle<()>>{
    let mut ck_used = vec![];
    let mut md5_used = vec![];
    let mut sha1_used = vec![];
    let mut sha224_used = vec![];
    let mut sha256_used = vec![];
    let mut sha384_used = vec![];
    let mut sha512_used = vec![];
    let mut b2_used = vec![];
    let mut cleaners = vec![];
    for source in sources.iter() {
        if let Some(ck) = source.ck {
            ck_used.push(format!("{:08x}", ck));
        }
        if let Some(md5) = source.md5 {
            md5_used.push(cksums::string_from(&md5));
        }
        if let Some(sha1) = source.sha1 {
            sha1_used.push(cksums::string_from(&sha1));
        }
        if let Some(sha224) = source.sha224 {
            sha224_used.push(cksums::string_from(&sha224));
        }
        if let Some(sha256) = source.sha256 {
            sha256_used.push(cksums::string_from(&sha256));
        }
        if let Some(sha384) = source.sha384 {
            sha384_used.push(cksums::string_from(&sha384));
        }
        if let Some(sha512) = source.sha512 {
            sha512_used.push(cksums::string_from(&sha512));
        }
        if let Some(b2) = source.b2 {
            b2_used.push(cksums::string_from(&b2));
        }
    }
    ck_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-ck", &ck_used)));
    md5_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-md5", &md5_used)));
    sha1_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha1", &sha1_used)));
    sha224_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha224", &sha224_used)));
    sha256_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha256", &sha256_used)));
    sha384_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha384", &sha384_used)));
    sha512_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-sha512", &sha512_used)));
    b2_used.sort_unstable();
    cleaners.push(thread::spawn(move ||
        remove_unused("sources/file-b2", &b2_used)));
    cleaners
}

fn clean_git_sources(sources: &Vec<Source>) {
    let hashes: Vec<u64> = sources.iter().map(
        |source| xxh3_64(source.url.as_bytes())).collect();
    let mut used: Vec<String> = hashes.iter().map(
        |hash| format!("{:016x}", hash)).collect();
    used.sort_unstable();
    remove_unused("sources/git", &used);
}

pub(crate) fn cleanup(netfile_sources: Vec<Source>, git_sources: Vec<Source>)
    -> Vec<JoinHandle<()>>
{
    let mut cleaners =
        clean_netfile_sources(&netfile_sources);
    cleaners.push(thread::spawn(move||clean_git_sources(&git_sources)));
    cleaners
}