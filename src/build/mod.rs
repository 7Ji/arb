mod builder;
mod dir;

use crate::{roots::BaseRoot, identity::IdentityActual};

use crate::pkgbuild::PKGBUILDs;

pub(crate) fn maybe_build(pkgbuilds: &PKGBUILDs, root: Option<BaseRoot>, actual_identity: &IdentityActual, 
    nobuild: bool, nonet: bool, sign: Option<&str>) -> Result<(), ()> 
{
    if let Some(_root) = root {
        if nobuild {
            return Ok(())
        }
        match crate::depend::split_pkgbuilds(pkgbuilds) {
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