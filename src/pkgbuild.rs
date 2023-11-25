// TODO: Split this into multiple modules
// Progress: already splitted part into pkgbuild/parse.rs, add mod parse to enable part of that

mod parse;
mod unused;

pub(crate) struct PKGBUILD {
    inner: parse::PkgbuildOwned,
    url: String,
    branch: String,
    commit: git2::Oid,
}

pub(crate) struct PKGBUILDs {
    inner: parse::PkgbuildsOwned
}

impl PKGBUILDs {
    fn from_config() {

    }
}