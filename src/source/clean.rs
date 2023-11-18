use std::{
        fs::{
            read_dir,
            remove_dir_all,
            remove_file,
        },
        path::Path,
        thread::{
            self,
            JoinHandle,
        }
    };
use xxhash_rust::xxh3::xxh3_64;
use crate::source::Source;

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
                    log::info!("Removing '{}' not used any more",
                        entry.path().display());
                    let _ = remove_dir_all(entry.path());
                }
                if metadata.is_file() || metadata.is_symlink() {
                    log::info!("Removing '{}' not used any more",
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
        if let Some(ck) = &source.ck {
            ck_used.push(ck.to_string());
        }
        if let Some(md5) = &source.md5 {
            md5_used.push(md5.to_string());
        }
        if let Some(sha1) = &source.sha1 {
            sha1_used.push(sha1.to_string());
        }
        if let Some(sha224) = &source.sha224 {
            sha224_used.push(sha224.to_string());
        }
        if let Some(sha256) = &source.sha256 {
            sha256_used.push(sha256.to_string());
        }
        if let Some(sha384) = &source.sha384 {
            sha384_used.push(sha384.to_string());
        }
        if let Some(sha512) = &source.sha512 {
            sha512_used.push(sha512.to_string());
        }
        if let Some(b2) = &source.b2 {
            b2_used.push(b2.to_string());
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