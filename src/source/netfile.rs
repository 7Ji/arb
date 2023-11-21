
use std::fs::{DirBuilder, create_dir};
use crate::{
        error::{
            Error,
            Result
        },
        filesystem::create_dir_allow_existing,
        source::{
            download,
            protocol::{
                NetfileProtocol,
                Protocol,
            },
            Proxy,
            Source,
        },
    };

pub(super) fn ensure_parents() -> Result<()>
{
    let mut path = std::path::PathBuf::from("sources");
    create_dir_allow_existing(&path)?;
    let mut name = String::from("file-");
    for integ in
        ["ck", "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "b2"]
    {
        name.push_str(integ);
        path.push(&name);
        create_dir_allow_existing(&path)?;
        if ! path.pop() {
            return Err(Error::ImpossibleLogic)
        }
        name.truncate(5);
    }
    Ok(())
}

fn optional_equal<C:PartialEq + std::fmt::Display>(a: &Option<C>, b: &Option<C>)
    -> bool
{
    if let Some(a) = a {
        if let Some(b) = b {
            if a == b {
                log::info!("Duplicated integrity checksum: '{}' == '{}'", a, b);
                return true
            }
        }
    }
    false
}

fn optional_update<C>(target: &mut Option<C>, source: &Option<C>)
-> Result<()>
    where C: PartialEq + Clone + std::fmt::Display
{
    if let Some(target) = target {
        if let Some(source) = source {
            if target != source {
                log::error!("Source target mismatch {} != {}, conflicting \
                    integ checksum in config, check your PKGBUILDs", 
                    source, target);
                return Err(Error::InvalidConfig);
            }
        }
    } else if let Some(source) = source {
        *target = Some(source.clone())
    }
    Ok(())
}

pub(super) fn push_source(
    sources: &mut Vec<Source>, source: &Source
) -> Result<()>
{
    let mut existing = None;
    for source_cmp in sources.iter_mut() {
        if optional_equal(
                &source_cmp.ck, &source.ck) ||
           optional_equal(
                &source_cmp.md5, &source.md5) ||
           optional_equal(
                &source_cmp.sha1, &source.sha1) ||
           optional_equal(
                &source_cmp.sha224, &source.sha224) ||
           optional_equal(
                &source_cmp.sha256, &source.sha256) ||
           optional_equal(
                &source_cmp.sha384, &source.sha384) ||
           optional_equal(
                &source_cmp.sha512, &source.sha512) ||
           optional_equal(&source_cmp.b2, &source.b2) {
            existing = Some(source_cmp);
            break;
        }
    }
    let existing = match existing {
        Some(existing) => existing,
        None => {
            sources.push(source.clone());
            return Ok(())
        },
    };
    optional_update(
        &mut existing.ck, &source.ck)?;
    optional_update(
        &mut existing.md5, &source.md5)?;
    optional_update(
        &mut existing.sha1, &source.sha1)?;
    optional_update(
        &mut existing.sha224, &source.sha224)?;
    optional_update(
        &mut existing.sha256, &source.sha256)?;
    optional_update(
        &mut existing.sha384, &source.sha384)?;
    optional_update(
        &mut existing.sha512, &source.sha512)?;
    optional_update(
        &mut existing.b2, &source.b2)
}

pub(super) fn download_source(
    source: &Source,
    integ_file: &super::cksums::IntegFile,
    actual_identity: &crate::identity::IdentityActual,
    skipint: bool,
    proxy: Option<&Proxy>
) -> Result<()>
{
    const MAX_TRIES: usize = 3;
    let protocol = 
        if let Protocol::Netfile{protocol} = &source.protocol{
            protocol
        } else {
            log::error!("Non-netfile source encountered by netfile cacher");
            return Err(Error::ImpossibleLogic)
        };
    let url = source.url.as_str();
    let mut proxy_actual = None;
    let mut max_tries = MAX_TRIES;
    let mut enable_proxy_at = MAX_TRIES;
    if let Some(proxy) = proxy {
        max_tries += proxy.after;
        enable_proxy_at = proxy.after
    };
    for i in 0..max_tries {
        if i == enable_proxy_at {
            if i > 0 {
                log::info!("Failed to download for {} times, using proxy", i);
            }
            proxy_actual = proxy.and_then(
                |proxy|Some(proxy.url.as_str()));
        }
        let integ_file_temp = integ_file.temp()?;
        log::info!("Downloading '{}' to '{}', try {} of {}",
            source.url, integ_file_temp.path.display(), i + 1, max_tries);
        if match &protocol {
            NetfileProtocol::File =>
                download::file(url, &integ_file_temp.path),
            NetfileProtocol::Ftp =>
                download::ftp(actual_identity, url, &integ_file_temp.path),
            NetfileProtocol::Http =>
                download::http(url, &integ_file_temp.path, proxy_actual),
            NetfileProtocol::Https =>
                download::http(url, &integ_file_temp.path, proxy_actual),
            NetfileProtocol::Rsync =>
                download::rsync(actual_identity, url, &integ_file_temp.path),
            NetfileProtocol::Scp =>
                download::scp(actual_identity, url, &integ_file_temp.path),
        }.is_ok() &&
            integ_file_temp.valid(skipint)
        {
            if integ_file.absorb(integ_file_temp).is_ok() {
                return Ok(())
            }
        }
    }
    log::error!("Failed to download netfile source '{}'", source.url);
    return Err(Error::IntegrityError)
}

pub(super) fn cache_source(
    source: &Source,
    integ_files: &Vec<super::cksums::IntegFile>,
    actual_identity: &crate::identity::IdentityActual,
    skipint: bool,
    proxy: Option<&Proxy>
) -> Result<()>
{
    assert!(integ_files.len() > 0, "No integ files");
    let mut good_files = vec![];
    let mut bad_files = vec![];
    for integ_file in integ_files.iter() {
        log::info!("Caching '{}' to '{}'",
            source.url,
            integ_file.get_path().display());
        if integ_file.valid(skipint) {
            good_files.push(integ_file);
        } else {
            bad_files.push(integ_file);
        }
    }
    let bad_count = bad_files.len();
    if bad_count > 0 {
        log::info!("Missing integ files for '{}': {}",
                source.url, bad_count);
    } else {
        log::info!("All integ files healthy for '{}'", source.url);
        return Ok(())
    }
    let mut bad_count = 0;
    while let Some(bad_file) = bad_files.pop() {
        let r = match good_files.last() {
            Some(good_file) =>
                bad_file.clone_file_from(good_file),
            None => download_source(
                source, bad_file, actual_identity, skipint, proxy),
        };
        match r {
            Ok(_) => good_files.push(bad_file),
            Err(_) => bad_count += 1,
        }
    }
    if bad_count > 0 {
        log::error!("Bad files still existing after download for '{}' ({})",
                    source.url, bad_count);
        Err(Error::IntegrityError)
    } else {
        Ok(())
    }
}