use reqwest::{
        ClientBuilder, 
        Proxy,
    };
use std::{
        fs::{
            File,
            hard_link,
            remove_file,
        },
        io::{
            Read,
            Write,
        },
        path::{
            Path,
            PathBuf
        },
        process::{
            Child,
            Command
        }, 
    };

const BUFFER_SIZE: usize = 0x400000; // 4M

pub(crate) fn clone_file(source: &Path, target: &Path) 
    -> Result<(), std::io::Error> 
{
    if target.exists() {
        if let Err(e) = remove_file(&target) {
            eprintln!("Failed to remove file {}: {}",
                &target.display(), e);
            return Err(e)
        }
    }
    match hard_link(&source, &target) {
        Ok(_) => return Ok(()),
        Err(e) => 
            eprintln!("Failed to link {} to {}: {}, trying heavy copy",
                        target.display(), source.display(), e),
    }
    let mut target_file = match File::create(&target) {
        Ok(target_file) => target_file,
        Err(e) => {
            eprintln!("Failed to open {} as write-only: {}",
                        target.display(), e);
            return Err(e)
        },
    };
    let mut source_file = match File::open(&source) {
        Ok(source_file) => source_file,
        Err(e) => {
            eprintln!("Failed to open {} as read-only: {}",
                        source.display(), e);
            return Err(e)
        },
    };
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let size_chunk = match
            source_file.read(&mut buffer) {
                Ok(size) => size,
                Err(e) => {
                    eprintln!("Failed to read file: {}", e);
                    return Err(e)
                },
            };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        match target_file.write_all(chunk) {
            Ok(_) => (),
            Err(e) => {
                eprintln!(
                    "Failed to write {} bytes into file '{}': {}",
                    size_chunk, target.display(), e);
                return Err(e);
            },
        }
    }
    println!("Cloned '{}' to '{}'", source.display(), target.display());
    Ok(())
}

pub(crate) fn file(url: &str, path: &Path) -> Result<(), ()> {
    if ! url.starts_with("file://") {
        eprintln!("URL '{}' does not start with file://", url);
        panic!("URL does not start with file://");
    }
    clone_file(&PathBuf::from(&url[7..]), path).or(Err(()))
}

fn wait_child(mut child: Child, job: &str) -> Result<(), ()> {
    let status = match child.wait() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to wait for child to {}: {}", &job, e);
            return Err(())
        },
    };
    match status.code() {
        Some(code) => {
            if code == 0 {
                return Ok(())
            } else {
                eprintln!("Child to {} bad return {}", &job, code);
                return Err(())
            }
        },
        None => {
            eprintln!("Child to {} has no return", &job);
            return Err(())
        },
    }
}

fn spawn_and_wait(command: &mut Command, job: &str) -> Result<(), ()> {
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!(
                "Failed to spawn child to {}: {}", &job, e);
            return Err(())
        },
    };
    wait_child(child, job)
}

pub(crate) fn ftp(url: &str, path: &Path) -> Result<(), ()> {
    let job = format!(
        "download FTP source from '{}' to '{}'", url, path.display());
    let command = Command::new("/usr/bin/curl")
        .arg("-qgfC")
        .arg("-")
        .arg("--ftp-pasv")
        .arg("--retry")
        .arg("3")
        .arg("--retry-delay")
        .arg("3")
        .arg("-o")
        .arg(path)
        .arg(url);
    spawn_and_wait(command, &job)
}

fn http_native(url: &str, path: &Path, proxy: Option<&str>) -> Result<(), ()> {
    let mut target = match File::create(path) {
        Ok(target) => target,
        Err(e) => {
            eprintln!("Failed to open {} as write-only: {}",
                        path.display(), e);
            return Err(())
        },
    };
    let future = async {
        let mut response = match match proxy {
            Some(proxy) => {
                let http_proxy = match Proxy::http(proxy) {
                    Ok(http_proxy) => http_proxy,
                    Err(e) => {
                        eprintln!("Failed to create http proxy '{}': {}", 
                                    proxy, e);
                        return Err(())
                    },
                };
                let https_proxy = match Proxy::https(proxy) 
                {
                    Ok(https_proxy) => https_proxy,
                    Err(e) => {
                        eprintln!("Failed to create https proxy '{}': {}", 
                                    proxy, e);
                        return Err(())
                    },
                };
                let client_builder = 
                    ClientBuilder::new()
                    .proxy(http_proxy)
                    .proxy(https_proxy);
                let client = match client_builder.build() {
                    Ok(client) => client,
                    Err(e) => {
                        eprintln!("Failed to build client: {}", e);
                        return Err(())
                    },
                };
                let request = match client.get(url).build() {
                    Ok(request) => request,
                    Err(e) => {
                        eprintln!("Failed to build request: {}", e);
                        return Err(())
                    },
                };
                client.execute(request).await
            },
            None => {
                reqwest::get(url).await
            },
        } {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Failed to get response from '{}': {}", url, e);
                return Err(())
            },
        };
        let time_start = tokio::time::Instant::now();
        let mut total = 0;
        loop {
            let chunk = match response.chunk().await {
                Ok(chunk) => chunk,
                Err(e) => {
                    eprintln!(
                        "Failed to get response chunk from '{}': {}", url, e);
                    return Err(())
                },
            };
            if let Some(chunk) = chunk {
                total += chunk.len();
                match target.write_all(&chunk) {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Failed to write to target file: {}", e);
                        return Err(())
                    },
                }
                let time_diff = 
                    (tokio::time::Instant::now() - time_start)
                        .as_secs_f64();
                if time_diff <= 0.0 {
                    continue;
                }
                let mut speed = total as f64 / time_diff;
                let suffixes = "BKMGTPEZY";
                let mut suffix_actual = ' ';
                for suffix in suffixes.chars() {
                    if speed >= 1024.00 {
                        speed /= 1024.00
                    } else {
                        suffix_actual = suffix;
                        break;
                    }
                }
                print!("Downloading {}: {:.2}{}/s\r", 
                        url, speed, suffix_actual);
            } else {
                break
            }
        }
        Ok(())
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn _http_curl(url: &str, path: &Path, proxy: Option<&str>) -> Result<(), ()> {
    let mut command = Command::new("/usr/bin/curl");
    if let Some(proxy) = proxy {
        command.env("http_proxy", proxy)
               .env("https_proxy", proxy);
    } else {
        command
            .env_remove("http_proxy")
            .env_remove("https_proxy");
    }
    command
        .arg("-qgb")
        .arg("")
        .arg("-fLC")
        .arg("-")
        .arg("--retry")
        .arg("3")
        .arg("--retry-delay")
        .arg("3")
        .arg("-o")
        .arg(path)
        .arg(url);
    let job = format!("download http(s) source from '{}' to '{}'",
                                url, path.display());
    spawn_and_wait(&mut command, &job)
}

pub(crate) fn http(url: &str, path: &Path, proxy: Option<&str>) 
    -> Result<(), ()>
{
    http_native(url, path, proxy)
}

pub(crate) fn rsync(url: &str, path: &Path) -> Result<(), ()> {
    let job = format!("download rsync source from '{}' to '{}'",
                                url, path.display());
    let command = Command::new("/usr/bin/rsync")
        .arg("--no-motd")
        .arg("-z")
        .arg(url)
        .arg(path);
    spawn_and_wait(command, &job)
}

pub(crate) fn scp(url: &str, path: &Path) -> Result<(), ()> {
    let job = format!("download scp source from '{}' to '{}'",
                                url, path.display());
    let command = Command::new("/usr/bin/scp")
        .arg("-C")
        .arg(url)
        .arg(path);
    spawn_and_wait(command, &job)
}
