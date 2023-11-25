mod builder;
mod dir;

use crate::error::Result;

pub(crate) fn maybe_build(
    pkgbuilds: &crate::pkgbuild::PKGBUILDs,
    root: Option<crate::root::BaseRoot>,
    actual_identity: &crate::identity::IdentityActual,
    nobuild: bool,
    nonet: bool,
    sign: &str
) -> Result<()>
{
    if let Some(_root) = root {
        if nobuild {
            return Ok(())
        }
        match crate::pacman::split_pkgbuilds(pkgbuilds) {
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