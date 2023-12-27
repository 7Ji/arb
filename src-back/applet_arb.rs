
use std::{collections::HashMap, ffi::OsString};

use clap::Parser;

use crate::{error::Result, source::{Proxy, git::Gmr}, identity::IdentityActual, config::{Pkgbuild, DepHash}};


struct Settings {
    actual_identity: IdentityActual,
    pkgbuilds_config: HashMap<String, Pkgbuild>,
    basepkgs: Vec<String>,
    proxy: Option<Proxy>,
    holdpkg: bool,
    holdgit: bool,
    skipint: bool,
    nobuild: bool,
    noclean: bool,
    nonet: bool,
    gmr: Option<Gmr>,
    dephash: DepHash,
    sign: String,
    homebinds: Vec<String>,
    terminal: bool
}

impl Settings {
    fn from_args<I, S>(args: I) -> Result<Self>
    where
        I: Iterator<Item = S>,
        S: Into<OsString> + Clone,
    {
        let arg: Args = clap::Parser::parse_from(args);
        let actual_identity = 
            crate::identity::IdentityActual::new_and_drop(&arg.drop)?;
        let mut config = crate::config::Config::from_file(arg.config)?;
        if ! arg.build.is_empty() {
            log::warn!("Only build the following packages: {:?}", arg.build);
            config.pkgbuilds.retain(|name, _|arg.build.contains(name));
        }
        let proxy = if ! arg.proxy.is_empty() {
            Some(Proxy::new(&arg.proxy, arg.lazyproxy))
        } else if ! config.proxy.is_empty() {
            Some(Proxy::new(&config.proxy, config.lazyproxy))
        } else {
            None
        };
        let gmr = if ! arg.gmr.is_empty() { 
            Some(Gmr::init(&arg.gmr))
        } else if ! config.gmr.is_empty() { 
            Some(Gmr::init(&config.gmr))
        } else {
            None
        };
        Ok(Settings {
            actual_identity,
            pkgbuilds_config: config.pkgbuilds,
            basepkgs: config.basepkgs,
            proxy,
            holdpkg: arg.holdpkg || config.holdpkg,
            holdgit: arg.holdgit || config.holdgit,
            skipint: arg.skipint || config.skipint,
            nobuild: arg.nobuild || config.nobuild,
            noclean: !arg.build.is_empty() || arg.noclean || config.noclean,
            nonet: arg.nonet || config.nonet,
            gmr,
            dephash: config.dephash,
            sign: if arg.sign.is_empty() { config.sign } else { arg.sign },
            homebinds: config.homebinds,
            terminal: is_terminal::is_terminal(std::io::stdout())
        })
    }

    fn work(&self) -> Result<()> {
        crate::filesystem::create_layout()?;
        let mut pkgbuilds =
            crate::pkgbuild::PKGBUILDs::from_config_healthy(
                &self.pkgbuilds_config, self.holdpkg,
                self.noclean, self.proxy.as_ref(),
                self.gmr.as_ref(), &self.homebinds, self.terminal
            )?;
        let root = pkgbuilds.prepare_sources(
            &self.actual_identity, &self.basepkgs, self.holdgit,
            self.skipint, self.noclean, self.proxy.as_ref(),
            self.gmr.as_ref(), &self.dephash, self.terminal)?;
        let r = crate::build::maybe_build(&pkgbuilds,
            root, &self.actual_identity, self.nobuild, self.nonet,
            &self.sign);
        let _ = std::fs::remove_dir("build");
        pkgbuilds.link_pkgs();
        if ! self.noclean {
            pkgbuilds.clean_pkgdir();
        }
        r
    }
}

pub(crate) fn main<I, S>(args: I) -> Result<()>
where
    I: Iterator<Item = S>,
    S: Into<OsString> + Clone,
{
    Settings::from_args(args)?.work()
}