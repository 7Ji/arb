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
        process::Command, 
    };

const BUFFER_SIZE: usize = 0x400000; // 4M

pub(crate) fn clone_file(source: &Path, target: &Path) {
    if target.exists() {
        match remove_file(&target) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to remove file {}: {}",
                    &target.display(), e);
                panic!("Failed to remove existing target file");
            },
        }
    }
    match hard_link(&source, &target) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Failed to link {} to {}: {}, trying heavy copy",
                        target.display(), source.display(), e);
            let mut target_file = match File::create(&target) {
                Ok(target_file) => target_file,
                Err(e) => {
                    eprintln!("Failed to open {} as write-only: {}",
                                target.display(), e);
                    panic!("Failed to open target file as write-only");
                },
            };
            let mut source_file = match File::open(&source) {
                Ok(source_file) => source_file,
                Err(e) => {
                    eprintln!("Failed to open {} as read-only: {}",
                                source.display(), e);
                    panic!("Failed to open source file as read-only");
                },
            };
            let mut buffer = vec![0; BUFFER_SIZE];
            loop {
                let size_chunk = match
                    source_file.read(&mut buffer) {
                        Ok(size) => size,
                        Err(e) => {
                            eprintln!("Failed to read file: {}", e);
                            panic!("Failed to read file");
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
                        panic!("Failed to write into target file");
                    },
                }
            }
        },
    }
    println!("Cloned '{}' to '{}'", source.display(), target.display());
}

pub(crate) fn file(url: &str, path: &Path) {
    if ! url.starts_with("file://") {
        eprintln!("URL '{}' does not start with file://", url);
        panic!("URL does not start with file://");
    }
    clone_file(&PathBuf::from(&url[7..]), path);
}

pub(crate) fn ftp(url: &str, path: &Path) {
    Command::new("/usr/bin/curl")
        .arg("-qgfC")
        .arg("-")
        .arg("--ftp-pasv")
        .arg("--retry")
        .arg("3")
        .arg("--retry-delay")
        .arg("3")
        .arg("-o")
        .arg(path)
        .arg(url)
        .spawn()
        .expect("Failed to run curl command to download ftp file")
        .wait()
        .expect("Failed to wait for spawned curl command");
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
        match response.chunk().await {
            Ok(chunk) => {
                while let Some(chunk) = &chunk {
                    match target.write_all(&chunk) {
                        Ok(_) => (),
                        Err(e) => {
                            eprintln!("Failed to write to target file: {}", e);
                            return Err(())
                        },
                    }
                }
            },
            Err(e) => {
                eprintln!("Failed to get response chunk from '{}': {}", url, e);
                return Err(())
            },
        }
        Ok(())
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn _http_curl(url: &str, path: &Path, proxy: Option<&str>) {
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
        .arg(url)
        .spawn()
        .expect("Failed to run curl command to download file")
        .wait()
        .expect("Failed to wait for spawned curl command");
}

pub(crate) fn http(url: &str, path: &Path, proxy: Option<&str>) {
    let _ = http_native(url, path, proxy);
}

pub(crate) fn rsync(url: &str, path: &Path) {
    Command::new("/usr/bin/rsync")
        .arg("--no-motd")
        .arg("-z")
        .arg(url)
        .arg(path)
        .spawn()
        .expect("Failed to run rsync command to download rsync file")
        .wait()
        .expect("Failed to wait for spawned rsync command");
}

pub(crate) fn scp(url: &str, path: &Path) {
    Command::new("/usr/bin/scp")
        .arg("-C")
        .arg(url)
        .arg(path)
        .spawn()
        .expect("Failed to run scp command to download scp file")
        .wait()
        .expect("Failed to wait for spawned scp command");
}
