use std::collections::HashMap;

use tempfile::tempdir;

mod depend;
mod pkgbuild;

pub(crate) use pkgbuild::PkgbuildConfig as PkgbuildConfig;

pub(crate) fn work(
    actual_identity: crate::identity::Identity,
    pkgbuilds_config: &HashMap<String, PkgbuildConfig>,
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
        pkgbuild::PKGBUILDs::from_config_healthy(
            pkgbuilds_config, holdpkg, noclean, proxy)?;
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