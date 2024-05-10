use std::{fs::File, io::{BufReader, Read}, path::{Path, PathBuf}};

// use blake2::Blake2b512;
use pkgbuild::{B2sum, Cksum, GitSourceFragment, Md5sum, Sha1sum, Sha224sum, Sha256sum, Sha384sum, Sha512sum};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
// use sha1::{digest::{generic_array::GenericArray, OutputSizeUser}, Digest};

use super::Pkgbuilds;

use crate::{checksum::Checksum, git::{RepoToOpen, ReposMap}, proxy::Proxy, Error, Result};

#[derive(Debug)]
struct GitSource {
    url: String,
    branches: Vec<String>,
    tags: Vec<String>,
}

impl Into<RepoToOpen> for &GitSource {
    fn into(self) -> RepoToOpen {
        RepoToOpen::new_with_url_parent_type(&self.url, "git")
    }
}

#[derive(PartialEq, Clone, Debug)]
enum CacheableProtocol {
    //Git,
    File,
    Ftp,
    Http,
    Https,
    Rsync,
}

#[derive(PartialEq, Clone, Debug)]
struct CachableUrl {
    protocol: CacheableProtocol,
    url: String,
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
    urls: Vec<CachableUrl>,
}

fn hex_from_option_bytearray<B: AsRef<[u8]>>(bytes: &Option<B>) -> String {
    if let Some(bytes) = bytes {
        bytes.as_ref().iter().map(|byte|format!("{:02x}", byte)).collect()
    } else {
        String::new()
    }
}

struct HashedFile {
    checksum: Checksum,
    path: PathBuf,
}

impl HashedFile {
    fn verify(&self) -> Result<bool> {
        self.checksum.verify_file(&self.path)
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
        let name_b2 = hex_from_option_bytearray(&self.b2sum);
        if self.b2sum.is_some() {

        }
        let name_sha512 = hex_from_option_bytearray(&self.sha512sum);
        let name_sha384 = hex_from_option_bytearray(&self.sha384sum);
        let name_sha256 = hex_from_option_bytearray(&self.sha256sum);
        let name_sha224 = hex_from_option_bytearray(&self.sha224sum);
        let name_sha1 = hex_from_option_bytearray(&self.sha1sum);
        let name_md5 = hex_from_option_bytearray(&self.md5sum);


        // Don't trust md5 and ck: only down-hashing to them, no up-hashing from
        // them
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
        let cacheable_url = CachableUrl { protocol, url };
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
        ReposMap::from_iter_into_repo_to_open(
            self.git.iter())?.sync(gmr, proxy, hold)
    }

    fn cache_hashed(&self, proxy: &Proxy, lazyint: bool) -> Result<()> {
        self.hashed.par_iter().try_for_each(
            |source|source.cache(proxy, lazyint))
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