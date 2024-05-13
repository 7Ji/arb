use std::{fs::File, io::{BufReader, Read}, path::{Path, PathBuf}};

// use blake2::Blake2b512;
use pkgbuild::{B2sum, Cksum, GitSourceFragment, Md5sum, Sha1sum, Sha224sum, Sha256sum, Sha384sum, Sha512sum};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
// use sha1::{digest::{generic_array::GenericArray, OutputSizeUser}, Digest};

use super::Pkgbuilds;

use crate::{checksum::Checksum, download::{download_file, download_ftp, download_http_https, download_rsync, download_scp}, filesystem::{clone_file, remove_file_allow_non_existing, remove_file_checked, rename_checked}, git::{RepoToOpen, ReposListToOpen, ReposMap}, proxy::Proxy, Error, Result};

#[derive(Debug)]
struct GitSource {
    url: String,
    branches: Vec<String>,
    tags: Vec<String>,
}

// impl Into<RepoToOpen> for &GitSource {
//     fn into(self) -> RepoToOpen {
//         RepoToOpen { 
//             path: format!("sources/git/").into(), 
//             url: self.url.clone(), 
//             branches: self.branches.clone(), 
//             tags: self.tags.clone() 
//         }
//         // RepoToOpen::new_with_url_parent_type(&self.url, "git")
//     }
// }

#[derive(PartialEq, Clone, Debug)]
enum CacheableProtocol {
    //Git,
    File,
    Ftp,
    Http,
    Https,
    Rsync,
    Scp,
}

#[derive(PartialEq, Clone, Debug)]
struct CacheableUrl {
    protocol: CacheableProtocol,
    url: String,
}

impl CacheableUrl {
    fn try_download_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        download_file(&self.url, path)
    }

    fn try_download_ftp<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        download_ftp(&self.url, path)
    }

    fn try_download_http_https<P: AsRef<Path>>(&self, path: P, proxy: &Proxy) 
        -> Result<()> 
    {
        download_http_https(&self.url, path, proxy)
    }

    fn try_download_rsync<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        download_rsync(&self.url, path)
    }

    fn try_download_scp<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        download_scp(&self.url, path)
    }

    fn try_download<P: AsRef<Path>>(&self, path: P, proxy: &Proxy) -> Result<()> {
        match self.protocol {
            CacheableProtocol::File => self.try_download_file(path),
            CacheableProtocol::Ftp => self.try_download_ftp(path),
            CacheableProtocol::Http | CacheableProtocol::Https
                => self.try_download_http_https(path, proxy),
            CacheableProtocol::Rsync => self.try_download_rsync(path),
            CacheableProtocol::Scp => self.try_download_scp(path),
        }
    }
}

#[derive(Debug)]
struct HashedSource {
    cksum: Option<Cksum>,
    md5sum: Option<Md5sum>,
    sha1sum: Option<Sha1sum>,
    sha224sum: Option<Sha224sum>,
    sha256sum: Option<Sha256sum>,
    sha384sum: Option<Sha384sum>,
    sha512sum: Option<Sha512sum>,
    b2sum: Option<B2sum>,
    urls: Vec<CacheableUrl>,
}
struct HashedFile {
    checksum: Checksum,
    path: PathBuf,
}

impl HashedFile {
    fn verify(&self) -> Result<bool> {
        self.checksum.verify_file(&self.path)
    }

    fn verify_another(&self, another: &Self) -> Result<bool> {
        self.checksum.verify_file(&another.path)
    }

    fn get_path_cache(&self) -> PathBuf {
        let mut os_string = self.path.clone().into_os_string();
        os_string.push(".cache");
        os_string.into()
    }

    fn absorb_another(&self, another: &Self) -> Result<()> {
        let path_cache = self.get_path_cache();
        clone_file(&another.path, &path_cache)?;
        rename_checked(&path_cache, &self.path)
    }

    fn absorb_good_files(&self, good_files: &Vec<Self>) -> Result<()> {
        for good_file in good_files.iter() {
            if let Ok(true) = self.verify_another(good_file) {
                if self.absorb_another(good_file).is_ok() {
                    return Ok(())
                }
            }
        }
        Err(Error::FilesystemConflict)
    }

    fn try_download(&self, cacheable_url: &CacheableUrl, proxy: &Proxy) 
        -> Result<()> 
    {
        let path_cache = self.get_path_cache();
        if let Err(e) = cacheable_url.try_download(&path_cache, proxy) {
            log::error!("Failed to download {} into cache file {}: {}",
                &cacheable_url.url, path_cache.display(), e);
            return Err(e)
        }
        if self.checksum.verify_file(&path_cache)? {
            rename_checked(&path_cache, &self.path)
        } else {
            Err(Error::IntegrityError)
        }
    }

    fn try_download_all(&self, urls: &Vec<CacheableUrl>, proxy: &Proxy) 
        -> Result<()>
    {
        let mut r = Ok(());
        for url in urls.iter() {
            r = self.try_download(url, proxy);
            if r.is_ok() {
                return Ok(())
            }
        }
        if let Err(e) = &r {
            log::error!("Failed to download into '{}' after trying all {} \
                URLs, last error: {}", self.path.display(), urls.len(), e);
        }
        r
    }

}

impl HashedSource {
    /// Cache this hashed source into local file(s) with hash value as keys.
    /// 
    /// If a file exists locally, check its integrity if `lazyint` is `false`,
    /// otherwise just assume its integrity.
    /// 
    /// If the source have multiple hashes, duplicate them first, then verify
    /// them, then download files if not trustworthy.
    /// 
    fn cache(&self, proxy: &Proxy, layint: bool) -> Result<()> {
        let mut good_files = Vec::new();
        let mut bad_files = Vec::new();
        macro_rules! push_good_bad {
            ($checksum: ident, $parent: literal, $type: expr) => {
                if let Some(checksum) = self.$checksum {
                    let mut name = $parent.to_string();
                    let checksum = $type(checksum.clone());
                    checksum.extend_string(&mut name);
                    let hashed_file = HashedFile {
                        checksum,
                        path: name.into()
                    };
                    match hashed_file.verify() {
                        Ok(true) => good_files.push(hashed_file),
                        _ => bad_files.push(hashed_file)
                    }
                }
            };
        }
        push_good_bad!(b2sum, "sources/file-b2/", Checksum::B2sum);
        push_good_bad!(sha512sum, "sources/file-sha512/", Checksum::Sha512sum);
        push_good_bad!(sha384sum, "sources/file-sha384/", Checksum::Sha384sum);
        push_good_bad!(sha256sum, "sources/file-sha256/", Checksum::Sha256sum);
        push_good_bad!(sha224sum, "sources/file-sha224/", Checksum::Sha224sum);
        push_good_bad!(sha1sum, "sources/file-sha1/", Checksum::Sha1sum);
        push_good_bad!(md5sum, "sources/file-md5/", Checksum::Md5sum);
        push_good_bad!(cksum, "sources/file-ck/", Checksum::Cksum);
        for bad_file in bad_files {
            if ! good_files.is_empty() {
                if let Err(e) = bad_file.absorb_good_files(&good_files) {
                    log::error!("Failed to absorb good hashed files into bad \
                        hashed file '{}': {}", bad_file.path.display(), e);
                } else {
                    good_files.push(bad_file);
                    continue
                }
            }
            if self.urls.is_empty() {
                log::error!("No URLs to fownload file from");
                return Err(Error::BrokenPKGBUILDs(Default::default()))
            }
            if let Err(e) = bad_file.try_download_all(
                &self.urls, proxy
            ) {
                log::error!("Failed to download into bad hashed file '{}' \
                    to convert it to good file: {}", 
                    bad_file.path.display(), e);
                return Err(e)
            }
            good_files.push(bad_file)
        }
        Ok(())
    }
}

#[derive(Default, Debug)]
pub(crate) struct CacheableSources {
    git: Vec<GitSource>,
    hashed: Vec<HashedSource>,
    // uncacheable: Vec<UncacheableSource>,
}

impl CacheableSources {
    fn add_git_source<S: AsRef<str>>(&mut self, url: S, fragment: &Option<GitSourceFragment>) {
        let url = url.as_ref();
        if let Some(git_source) = self.git.iter_mut().find(
            |git|git.url == url) 
        {
            if git_source.branches.is_empty() && git_source.tags.is_empty() {
                return // This git source needs everything
            }
            match fragment {
                Some(GitSourceFragment::Branch(branch)) => 
                    git_source.branches.push(branch.into()),
                Some(GitSourceFragment::Tag(tag)) => 
                    git_source.tags.push(tag.into()),
                _ => {
                    git_source.branches.clear();
                    git_source.tags.clear();
                },
            }
        } else {
            self.git.push(
                match fragment {
                    Some(GitSourceFragment::Branch(branch)) => 
                        GitSource {
                            url: url.into(), 
                            branches: vec![branch.into()], 
                            tags: Vec::new(),
                        },
                    Some(GitSourceFragment::Tag(tag)) => 
                        GitSource {
                            url: url.into(), 
                            branches: Vec::new(), 
                            tags: vec![tag.into()],
                        },
                    _ => GitSource { 
                            url: url.into(), 
                            branches: Vec::new(), 
                            tags: Vec::new()
                    },
                }
            )
            
        }
    }

    fn add_hashed_source<S: Into<String>>(&mut self, 
        url: S, protocol: CacheableProtocol,
        cksum: &Option<Cksum>,
        md5sum: &Option<Md5sum>,
        sha1sum: &Option<Sha1sum>,
        sha224sum: &Option<Sha224sum>,
        sha256sum: &Option<Sha256sum>,
        sha384sum: &Option<Sha384sum>,
        sha512sum: &Option<Sha512sum>,
        b2sum: &Option<B2sum>
    ) {
        let url = url.into();
        if cksum.is_none() && md5sum.is_none() && sha1sum.is_none() && 
            sha224sum.is_none() && sha256sum.is_none() && sha384sum.is_none() &&
            sha512sum.is_none() && b2sum.is_none()
        {
            log::warn!("Source '{}' is not hashed", url);
            return
        }
        let cacheable_url = CacheableUrl { protocol, url };
        if let Some(hashed_source) = 
            self.hashed.iter_mut().find(|hashed|{
                (hashed.b2sum.is_some() && hashed.b2sum == *b2sum) ||
                (hashed.sha512sum.is_some() && hashed.sha512sum == *sha512sum) ||
                (hashed.sha384sum.is_some() && hashed.sha384sum == *sha384sum) ||
                (hashed.sha256sum.is_some() && hashed.sha256sum == *sha256sum) ||
                (hashed.sha224sum.is_some() && hashed.sha224sum == *sha224sum) ||
                (hashed.sha1sum.is_some() && hashed.sha1sum == *sha1sum) ||
                (hashed.md5sum.is_some() && hashed.md5sum == *md5sum) ||
                (hashed.cksum.is_some() && hashed.cksum == *cksum)
            }) 
        {
            if ! hashed_source.urls.contains(&cacheable_url) {
                hashed_source.urls.push(cacheable_url.clone())
            }
            macro_rules! replace_none {
                ($suffix: ident) => {
                    if hashed_source.$suffix.is_none() {
                        if let Some(filler) = $suffix {
                            hashed_source.$suffix.replace(filler.clone());
                        } 
                    }
                };
                ($($suffix: ident), +) => {
                    $(
                        replace_none!($suffix);
                    )+
                };
            }
            replace_none!(b2sum, sha512sum, sha384sum, sha256sum, sha224sum, 
                        sha1sum, md5sum, cksum);
        } else {
            self.hashed.push(HashedSource { 
                cksum: cksum.clone(), 
                md5sum: md5sum.clone(), 
                sha1sum: sha1sum.clone(), 
                sha224sum: sha224sum.clone(),
                sha256sum: sha256sum.clone(),
                sha384sum: sha384sum.clone(),
                sha512sum: sha512sum.clone(),
                b2sum: b2sum.clone(),
                urls: vec![cacheable_url] }) 
        }

    }

    fn cache_git(&self, gmr: &str, proxy: &Proxy, hold: bool) -> Result<()> {
        log::info!("Caching git sources...");
        let mut repos_list = ReposListToOpen::default();
        for repo in self.git.iter() {
            repos_list.add("git", &repo.url, &repo.branches, &repo.tags);
        }
        repos_list.try_open_init_into_map()?.sync(gmr, proxy, hold)
    }

    fn cache_hashed(&self, proxy: &Proxy, lazyint: bool) -> Result<()> {
        let results: Vec<Result<()>> = self.hashed.par_iter().map(
            |source|source.cache(
                        proxy, lazyint)
                    ).collect();
        for result in results {
            if result.is_err() {
                return result;
            }
        }
        Ok(())
    }

    pub(crate) fn cache(
        &self, gmr: &str, proxy: &Proxy, holdgit: bool, lazyint: bool
    ) -> Result<()> 
    {
        self.cache_git(gmr, proxy, holdgit)?;
        self.cache_hashed(proxy, lazyint)
    }

    pub(crate) fn git_urls(&self) -> Vec<String> {
        self.git.iter().map(
            |git|git.url.clone()).collect()
    }
}

impl From<&Pkgbuilds> for CacheableSources {
    fn from(pkgbuilds: &Pkgbuilds) -> Self {
        let mut cacheable_sources = Self::default();
        for pkgbuild in pkgbuilds.pkgbuilds.iter() {
            for source_with_checksum in 
                pkgbuild.inner.sources_with_checksums() 
            {
                let source = &source_with_checksum.source;
                let url = &source.url;
                let protocol = match &source.protocol {
                    pkgbuild::SourceProtocol::File => CacheableProtocol::File,
                    pkgbuild::SourceProtocol::Ftp => CacheableProtocol::Ftp,
                    pkgbuild::SourceProtocol::Http => CacheableProtocol::Http,
                    pkgbuild::SourceProtocol::Https => CacheableProtocol::Https,
                    pkgbuild::SourceProtocol::Rsync => CacheableProtocol::Rsync,
                    pkgbuild::SourceProtocol::Scp => CacheableProtocol::Scp,
                    pkgbuild::SourceProtocol::Git { fragment, signed: _ } => {
                        cacheable_sources.add_git_source(url, fragment);
                        continue
                    },
                    _ => continue,
                };
                cacheable_sources.add_hashed_source(url, protocol, 
                    &source_with_checksum.cksum,
                    &source_with_checksum.md5sum,
                    &source_with_checksum.sha1sum,
                    &source_with_checksum.sha224sum,
                    &source_with_checksum.sha256sum,
                    &source_with_checksum.sha384sum,
                    &source_with_checksum.sha512sum,
                    &source_with_checksum.b2sum
                )
            }
        }
        cacheable_sources.git.sort_unstable_by(|some, other|some.url.cmp(&other.url));
        cacheable_sources
    }
}