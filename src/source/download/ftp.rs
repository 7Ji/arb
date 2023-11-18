pub(crate) fn ftp(
    actual_identity: &crate::identity::IdentityActual,
    url: &str,
    path: &std::path::Path
) -> Result<(), ()>
{
    let job = format!(
        "download FTP source from '{}' to '{}'", url, path.display());
    let mut command = std::process::Command::new("/usr/bin/curl");
    actual_identity.set_root_drop_command(
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
            .arg(url));
    crate::child::output_and_check(&mut command, &job)
}