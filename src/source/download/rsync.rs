pub(crate) fn rsync(
    actual_identity: &crate::identity::IdentityActual,
    url: &str, 
    path: &std::path::Path
) -> Result<(), ()> 
{
    let job = format!("download rsync source from '{}' to '{}'",
                                url, path.display());
    let mut command = std::process::Command::new("/usr/bin/rsync");
    actual_identity.set_root_drop_command(
        command
            .arg("--no-motd")
            .arg("-z")
            .arg(url)
            .arg(path));
    crate::child::output_and_check(&mut command, &job)
}