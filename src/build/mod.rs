use std::collections::HashMap;

mod builder;
mod depend;
mod dir;
mod pkgbuild;
mod sign;

pub(crate) use pkgbuild::PkgbuildConfig as PkgbuildConfig;
pub(crate) use depend::DepHashStrategy as DepHashStrategy;

pub(crate) fn work(
    actual_identity: crate::identity::IdentityActual,
    pkgbuilds_config: &HashMap<String, PkgbuildConfig>,
    basepkgs: &Vec<String>,
    proxy: Option<&str>,
    holdpkg: bool,
    holdgit: bool,
    skipint: bool,
    nobuild: bool,
    noclean: bool,
    nonet: bool,
    gmr: Option<&str>,
    dephash_strategy: &DepHashStrategy,
    sign: Option<&str>
) -> Result<(), ()>
{
    dir::prepare_updated_latest_dirs()?;
    let gmr = gmr.and_then(|gmr|
        Some(crate::source::git::Gmr::init(gmr)));
    let mut pkgbuilds = 
        pkgbuild::PKGBUILDs::from_config_healthy(
            pkgbuilds_config, holdpkg, noclean, proxy, gmr.as_ref())?;
    let r = match pkgbuilds.prepare_sources(
        &actual_identity, basepkgs, holdgit, 
        skipint, noclean, proxy, gmr.as_ref(), dephash_strategy)? 
    {
        Some(_root) => {
            if nobuild {
                Ok(())
            } else {
                builder::build_any_needed(
                    &mut pkgbuilds, &actual_identity, nonet, sign)
            }
        },
        None => {
            println!("No need to build any packages");
            Ok(())
        },
    };
    let _ = std::fs::remove_dir("build");
    pkgbuilds.link_pkgs();
    if ! noclean {
        pkgbuilds.clean_pkgdir();
    }
    r
}