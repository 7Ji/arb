pub(crate) fn scp(
    actual_identity: &crate::identity::IdentityActual,
    url: &str, 
    path: &std::path::Path
) -> Result<(), ()> 
{
    let job = format!("download scp source from '{}' to '{}'",
                                url, path.display());
    let mut command = std::process::Command::new("/usr/bin/scp");
    actual_identity.set_root_drop_command(
        command
            .arg("-C")
            .arg(url)
            .arg(path));
    crate::child::output_and_check(&mut command, &job)
}