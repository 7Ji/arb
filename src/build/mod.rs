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
    let gmr = gmr.and_then(|gmr|
        Some(crate::source::git::Gmr::init(gmr)));
    let mut pkgbuilds = 
        pkgbuild::PKGBUILDs::from_config_healthy(
            pkgbuilds_config, holdpkg, noclean, proxy, gmr.as_ref())?;
    let pkgbuilds_dir =
        tempdir().expect("Failed to create temp dir to dump PKGBUILDs");
    match pkgbuilds.prepare_sources(&actual_identity, 
        &pkgbuilds_dir, holdgit, skipint, noclean, proxy, gmr.as_ref())? 
    {
        Some(_root) => {
            if ! nobuild {
                pkgbuilds.build_any_needed(&actual_identity, nonet, sign)?
            }
        },
        None => {
            println!("No need to build any packages");
        },
    };
    if ! noclean {
        pkgbuilds.clean_pkgdir();
    }
    Ok(())
}