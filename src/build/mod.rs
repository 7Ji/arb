use tempfile::tempdir;

mod depend;
mod pkgbuild;

pub(crate) fn work<P: AsRef<std::path::Path>>(
    actual_identity: crate::identity::Identity,
    pkgbuilds_yaml: P,
    proxy: Option<&str>,
    holdpkg: bool,
    holdgit: bool,
    skipint: bool,
    nobuild: bool,
    noclean: bool,
    nonet: bool,
    gmr: Option<&str>,
    sign: Option<&str>
) -> Result<(), ()>
{
    let gmr = match gmr {
        Some(gmr) => Some(crate::source::git::Gmr::init(gmr)),
        None => None,
    };
    let mut pkgbuilds = 
        pkgbuild::PKGBUILDs::from_yaml_config_healthy(
            &pkgbuilds_yaml, holdpkg, noclean, proxy).ok_or(())?;
    let pkgbuilds_dir =
        tempdir().expect("Failed to create temp dir to dump PKGBUILDs");
    let _base_root = pkgbuilds.prepare_sources(&actual_identity, 
        &pkgbuilds_dir, holdgit, skipint, noclean, proxy, gmr.as_ref())?;
    if nobuild {
        return Ok(());
    }
    pkgbuilds.build_any_needed(&actual_identity, nonet, sign)?;
    if noclean {
        return Ok(());
    }
    pkgbuilds.clean_pkgdir();
    Ok(())
}