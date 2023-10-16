use std::collections::HashMap;

mod builder;
mod depend;
mod dir;
mod interdep;
mod pkgbuild;
mod sign;

pub(crate) use pkgbuild::PkgbuildConfig as PkgbuildConfig;
pub(crate) use depend::DepHashStrategy as DepHashStrategy;

use crate::{roots::BaseRoot, identity::IdentityActual, source::Proxy};

use self::pkgbuild::PKGBUILDs;

fn maybe_build(pkgbuilds: &PKGBUILDs, root: Option<BaseRoot>, actual_identity: &IdentityActual, 
    nobuild: bool, nonet: bool, sign: Option<&str>) -> Result<(), ()> 
{
    if let Some(_root) = root {
        if nobuild {
            return Ok(())
        }
        match interdep::split_pkgbuilds(pkgbuilds) {
            Ok(layers) => {
                for layer in layers {
                    builder::build_any_needed_layer(
                        &layer, &actual_identity, nonet, sign)?

                }
            },
            Err(_) => builder::build_any_needed(
                        &pkgbuilds, &actual_identity, nonet, sign)?,
        }
    }
    Ok(())
}

pub(crate) fn work(
    actual_identity: crate::identity::IdentityActual,
    pkgbuilds_config: &HashMap<String, PkgbuildConfig>,
    basepkgs: &Vec<String>,
    proxy: Option<&Proxy>,
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
    let root = pkgbuilds.prepare_sources(
        &actual_identity, basepkgs, holdgit, 
        skipint, noclean, proxy, gmr.as_ref(), dephash_strategy)?;
    let r = maybe_build(&pkgbuilds,
        root, &actual_identity, nobuild, nonet, sign);
    let _ = std::fs::remove_dir("build");
    pkgbuilds.link_pkgs();
    if ! noclean {
        pkgbuilds.clean_pkgdir();
    }
    r
}