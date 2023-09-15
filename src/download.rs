use std::{
        path::Path, 
        process::Command
    };

pub(crate) fn file(url: &str, path: &Path) {
    Command::new("/usr/bin/curl")
        .env_clear()
        .arg("-qgC")
        .arg("-")
        .arg("-o")
        .arg(path)
        .arg(url)
        .spawn()
        .expect("Failed to run curl command to download file")
        .wait()
        .expect("Failed to wait for spawned curl command");
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

pub(crate) fn http(url: &str, path: &Path, proxy: Option<&str>) {
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
