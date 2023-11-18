use std::{
        collections::HashMap,
        str::FromStr,
    };

mod aur;
mod cache;
mod cksums;
mod clean;
mod download;
mod extract;
pub(crate) mod git;
mod protocol;
mod netfile;
mod parse;
mod proxy;

use cksums::{
    IntegFile,
    Cksum,
    Md5sum,
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
    B2sum,
};

use protocol::{
    Protocol,
    VcsProtocol,
};

pub(crate) use parse::{
    get_sources,
    unique_sources
};

pub(crate) use cache::cache_sources_mt;
pub(crate) use clean::{
    cleanup,
    remove_unused,
};
pub(crate) use extract::extract;
pub(crate) use proxy::Proxy;

#[derive(Clone)]
pub(crate) struct Source {
    name: String,
    protocol: Protocol,
    url: String,
    hash_url: u64,
    ck: Option<Cksum>,     // 32-bit CRC
    md5: Option<Md5sum>,   // 128-bit MD5
    sha1: Option<Sha1sum>,  // 160-bit SHA-1
    sha224: Option<Sha224sum>,// 224-bit SHA-2
    sha256: Option<Sha256sum>,// 256-bit SHA-2
    sha384: Option<Sha384sum>,// 384-bit SHA-2
    sha512: Option<Sha512sum>,// 512-bit SHA-2
    b2: Option<B2sum>,    // 512-bit Blake-2B
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
            let domain = xxhash_rust::xxh3::xxh3_64(
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