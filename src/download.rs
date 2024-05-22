use std::{path::Path, io::Read};

use ureq::{Request, Response};

use crate::{child::{command_new_no_stdin, spawn_and_wait}, filesystem::{clone_file, file_create_checked}, io::reader_to_writer, proxy::Proxy, Error, Result};

const TRIES: usize = 3;
const TRIES_STR: &str = "3";

pub(crate) fn download_file<S, P>(url: S, path: P) -> Result<()> 
where
    S: AsRef<str>,
    P: AsRef<Path>,
{
    let url = url.as_ref();
    let path = path.as_ref();
    log::info!("Downloading '{}' (protocol file) to '{}'", url, path.display());
    if url.starts_with("file://") {
        clone_file(&url[7..], path)
    } else {
        log::error!("URL '{}' is not in file protocol", url);
        Err(Error::BrokenPKGBUILDs(Default::default()))
    }
}

pub(crate) fn download_ftp<S, P>(url:S, path: P) -> Result<()> 
where
    S: AsRef<str>,
    P: AsRef<Path>,
{
    let url = url.as_ref();
    let path = path.as_ref();
    log::info!("Downloading '{}' (protocol ftp) to '{}'", url, path.display());
    spawn_and_wait(
        command_new_no_stdin("curl")
            .arg("-qgfC")
            .arg("-")
            .arg("--ftp-pasv")
            .arg("--retry")
            .arg(TRIES_STR)
            .arg("--retry-delay")
            .arg("3")
            .arg("-o")
            .arg(path)
            .arg(url))
}

fn response_to_file<P: AsRef<Path>>(response: Response, path: P) 
    -> Result<()> 
{
    let len = match response.header("content-length") {
        Some(len) => match len.parse() {
            Ok(len) => len,
            Err(e) => {
                log::error!("Failed to parse header content-length: {}", e);
                return Err(Error::InvalidArgument)
            },
        },
        None => {
            log::info!("Warning: response does not have 'content-length', \
                limit max download size to 4GiB");
            0x100000000
        }
    };
    reader_to_writer(
        &mut response.into_reader().take(len), 
        &mut file_create_checked(path)?
    )
}

fn request_to_file<P: AsRef<Path>>(request: Request, path: P) 
    -> Result<()> 
{
    match request.call() {
        Ok(response) => response_to_file(response, path),
        Err(e) => {
            log::error!("Failed to call request: {}", e);
            Err(e.into())
        },
    }
}

pub(crate) fn download_http_https<S, P>(url: S, path: P, proxy: &Proxy) 
    -> Result<()> 
where
    S: AsRef<str>,
    P: AsRef<Path>,
{
    let url = url.as_ref();
    let path = path.as_ref();
    log::info!("Downloading '{}' (protocol http(s)) to '{}'", 
        url, path.display());
    let (tries_without, tries_with) = 
        proxy.tries_without_and_with(3);
    for _ in 0..tries_without {
        if request_to_file(ureq::get(url), path).is_ok() {
            return Ok(())
        }
    }
    if tries_with == 0 {
        log::error!("Failed to download '{}', no proxy to retry", url);
        return Err(Error::IntegrityError)
    }
    let proxy_url = proxy.get_url();
    let proxy_opt = match ureq::Proxy::new(proxy_url) {
        Ok(proxy_opt) => proxy_opt,
        Err(e) => {
            log::error!("Failed to create proxy from '{}': {}", 
                        proxy_url, e);
            return Err(e.into())
        },
    };
    let agent = ureq::AgentBuilder::new().proxy(proxy_opt).build();
    for _ in 0..tries_with {
        if request_to_file(agent.get(url), path).is_ok() {
            return Ok(())
        }
    }
    log::error!("Failed to download '{}' even with proxy", url);
    return Err(Error::IntegrityError)
    
}

pub(crate) fn download_rsync<S, P>(url: S, path: P) -> Result<()> 
where
    S: AsRef<str>,
    P: AsRef<Path>,
{
    let url = url.as_ref();
    let path = path.as_ref();
    log::info!("Downloading '{}' (protocol ftp) to '{}'", url, path.display());
    spawn_and_wait(
        command_new_no_stdin("rsync")
            .arg("--no-motd")
            .arg("-z")
            .arg(url)
            .arg(path))
}

pub(crate) fn download_scp<S, P>(url: S, path: P) -> Result<()> 
where
    S: AsRef<str>,
    P: AsRef<Path>,
{
    let url = url.as_ref();
    let path = path.as_ref();
    log::info!("Downloading '{}' (protocol ftp) to '{}'", url, path.display());
    spawn_and_wait(
        command_new_no_stdin("scp")
            .arg("-C")
            .arg(url)
            .arg(path))
}