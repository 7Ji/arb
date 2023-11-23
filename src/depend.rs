mod db;
mod depends;
mod interdep;
mod paconfig;

#[derive(Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DepHashStrategy {
    Strict, // dep + makedep
    Loose,  // dep
    None,   // none
}

impl Default for DepHashStrategy {
    fn default() -> Self {
        Self::None
    }
}

pub(crate) use db::DbHandle;
pub(crate) use depends::Depends;
pub(crate) use interdep::split_pkgbuilds;