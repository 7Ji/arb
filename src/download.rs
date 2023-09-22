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
        .env_clear()
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

fn http_native(url: &str, path: &Path, proxy: Option<&str>) {
    let mut target = match File::create(path) {
        Ok(target) => target,
        Err(e) => {
            eprintln!("Failed to open {} as write-only: {}",
                        path.display(), e);
            panic!("Failed to open target file as write-only");
        },
    };
    let future = async {
        let mut response = match match proxy {
            Some(proxy) => {
                let client_builder = 
                    ClientBuilder::new()
                    .proxy(Proxy::https(proxy)
                    .expect("Failed to create https proxy"))
                    .proxy(Proxy::http(proxy)
                    .expect("Failed to create http proxy"));
                let client = 
                    client_builder.build()
                    .expect("Failed to build client");
                let request = 
                    client.get(url).build()
                    .expect("Failed to build request");
                client.execute(request).await
            },
            None => {
                reqwest::get(url).await
            },
        } {
            Ok(response) => response,
            Err(e) => {
                eprintln!("Failed to get response from '{}': {}", url, e);
                return
            },
        };
        match response.chunk().await {
            Ok(chunk) => {
                while let Some(chunk) = &chunk {
                    target.write_all(&chunk)
                        .expect("Failed to write to file");
                }
            },
            Err(e) => {
                eprintln!("Failed to get response chunk from '{}': {}", url, e);
                return
            },
        }
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

fn http_curl(url: &str, path: &Path, proxy: Option<&str>) {
    let mut command = Command::new("/usr/bin/curl");
    command.env_clear();
    if let Some(proxy) = proxy {
        command.env("http_proxy", proxy)
               .env("https_proxy", proxy);
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

pub(crate) fn http(url: &str, path: &Path, proxy: Option<&str>, native: bool) {
    if native {
        http_native(url, path, proxy);
    } else {
        http_curl(url, path, proxy);
    }
}

pub(crate) fn rsync(url: &str, path: &Path) {
    Command::new("/usr/bin/rsync")
        .env_clear()
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
        .env_clear()
        .arg("-C")
        .arg(url)
        .arg(path)
        .spawn()
        .expect("Failed to run scp command to download scp file")
        .wait()
        .expect("Failed to wait for spawned scp command");
}
