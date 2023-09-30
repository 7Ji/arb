pub(crate) fn ftp(url: &str, path: &std::path::Path) -> Result<(), ()> {
    let job = format!(
        "download FTP source from '{}' to '{}'", url, path.display());
    let mut command = 
        std::process::Command::new("/usr/bin/curl");
    command
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
    super::common::spawn_and_wait(&mut command, &job)
}