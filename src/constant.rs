//! Crate-level paths, paths that are system-related is not here but in modules

pub(crate) const PATH_BUILD: &str = "build";
pub(crate) const PATH_LOGS: &str = "logs";
pub(crate) const PATH_PKGS: &str = "pkgs";
pub(crate) const PATH_SOURCES: &str = "sources";
pub(crate) const PATH_PACMAN_SYNC: &str = "build/pacman.sync";
pub(crate) const PATH_PKGS_UPDATED: &str = "pkgs/updated";
pub(crate) const PATH_PKGS_LATEST: &str = "pkgs/latest";
pub(crate) const PATH_PKGS_CACHE: &str = "pkgs/cache";

pub(crate) const PATH_ROOT_SINGLE_BASE: &str = "build/root.single.base";
pub(crate) const PATH_PKGBUILDS: &str = "build/PKGBUILDs";
pub(crate) const PATH_PACMAN_CONF: &str = "/etc/pacman.conf";
pub(crate) const PATH_PACMAN_CONF_UNDER_ROOT: &str = "etc/pacman.conf";
pub(crate) const PATH_SOURCES_GIT: &str = "sources/git";
pub(crate) const PATH_SOURCES_PKGBUILD: &str = "sources/PKGBUILD";
pub(crate) const PATH_SOURCES_FILE_B2: &str = "sources/file-b2";
pub(crate) const PATH_SOURCES_FILE_SHA512: &str = "sources/file-sha512";
pub(crate) const PATH_SOURCES_FILE_SHA384: &str = "sources/file-sha384";
pub(crate) const PATH_SOURCES_FILE_SHA256: &str = "sources/file-sha256";
pub(crate) const PATH_SOURCES_FILE_SHA224: &str = "sources/file-sha224";
pub(crate) const PATH_SOURCES_FILE_SHA1: &str = "sources/file-sha1";
pub(crate) const PATH_SOURCES_FILE_MD5: &str = "sources/file-md5";
pub(crate) const PATH_SOURCES_FILE_CK: &str = "sources/file-ck";