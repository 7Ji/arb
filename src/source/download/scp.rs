pub(crate) fn scp(url: &str, path: &std::path::Path) -> Result<(), ()> {
    let job = format!("download scp source from '{}' to '{}'",
                                url, path.display());
    let mut command = 
        std::process::Command::new("/usr/bin/scp");
    command
        .arg("-C")
        .arg(url)
        .arg(path);
    super::child::spawn_and_wait(&mut command, &job)
}