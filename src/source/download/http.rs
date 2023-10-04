use reqwest::{
    ClientBuilder, 
    Proxy,
};

use std::{
    fs::File,
    io::Write,
    path::Path,
    process::Command,
};

pub(crate) fn http_native(url: &str, path: &Path, proxy: Option<&str>) 
    -> Result<(), ()> 
{
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

fn _http_curl(
    actual_identity: &crate::identity::Identity,
    url: &str, 
    path: &Path, 
    proxy: Option<&str>
) -> Result<(), ()> 
{
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
    actual_identity.set_root_drop_command(&mut command);
    let job = format!("download http(s) source from '{}' to '{}'",
                                url, path.display());
    super::child::spawn_and_wait(&mut command, &job)
}