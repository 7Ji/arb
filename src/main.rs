mod build;
mod child;
mod config;
mod depend;
mod filesystem;
mod identity;
mod pkgbuild;
mod root;
mod sign;
mod source;
mod threading;

use std::collections::HashMap;

struct Settings {
    actual_identity: crate::identity::IdentityActual,
    pkgbuilds_config: HashMap<String, config::Pkgbuild>,
    basepkgs: Vec<String>,
    proxy: Option<source::Proxy>,
    holdpkg: bool,
    holdgit: bool,
    skipint: bool,
    nobuild: bool,
    noclean: bool,
    nonet: bool,
    gmr: Option<String>,
    dephash_strategy: config::DepHashStrategy,
    sign: Option<String>,
    home_binds: Vec<String>,
    terminal: bool
}

fn log_setup() {
    env_logger::Builder::from_env(
        env_logger::Env::default().filter_or(
            "ARB_LOG_LEVEL", "info")
        ).target(env_logger::Target::Stdout).init();
}

fn prepare() -> Result<Settings, &'static str> {
    log_setup();
    let arg: config::Arg = clap::Parser::parse();
    let actual_identity =
    identity::IdentityActual::new_and_drop(arg.drop.as_deref())
        .or_else(|_|Err("Failed to get actual identity"))?;
    let mut config: config::Config = serde_yaml::from_reader(
        std::fs::File::open(&arg.config).or_else(
        |e|{
            log::error!("Failed to open config file '{}': {}", arg.config, e);
            Err("Failed to open config file")
        })?)
    .or_else(
    |e|{
        log::error!("Failed to parse YAML: {}", e);
        Err("Failed to parse YAML config")
    })?;
    if ! arg.build.is_empty() {
        log::warn!("Only build the following packages: {:?}", arg.build);
        config.pkgbuilds.retain(|name, _|arg.build.contains(name));
    }
    let proxy = source::Proxy::from_str_usize(
        arg.proxy.as_deref().or(config.proxy.as_deref()),
        match arg.proxy_after {
            Some(proxy_after) => proxy_after,
            None => match config.proxy_after {
                Some(proxy_after) => proxy_after,
                None => 0,
            },
        });
    filesystem::prepare_updated_latest_dirs().or_else(
        |_|Err("Failed to prepare pkgs/{updated,latest}"))?;
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
        gmr: arg.gmr.or(config.gmr),
        dephash_strategy: config.dephash_strategy,
        sign: arg.sign.or(config.sign),
        home_binds: config.home_binds,
        terminal: is_terminal::is_terminal(std::io::stdout())
    })
}

fn work(settings: Settings) -> Result<(), &'static str> {
    let gmr = settings.gmr.and_then(|gmr|
        Some(crate::source::git::Gmr::init(gmr.as_str())));
    let mut pkgbuilds =
        pkgbuild::PKGBUILDs::from_config_healthy(
            &settings.pkgbuilds_config, settings.holdpkg,
            settings.noclean, settings.proxy.as_ref(),
            gmr.as_ref(), &settings.home_binds, settings.terminal
        ).or_else(|_|Err("Failed to prepare PKGBUILDs list"))?;
    let root = pkgbuilds.prepare_sources(
        &settings.actual_identity, &settings.basepkgs, settings.holdgit,
        settings.skipint, settings.noclean, settings.proxy.as_ref(),
        gmr.as_ref(), &settings.dephash_strategy, settings.terminal
        ).or_else(|_|Err("Failed to prepare sources"))?;
    let r = build::maybe_build(&pkgbuilds,
        root, &settings.actual_identity, settings.nobuild, settings.nonet,
         settings.sign.as_deref());
    let _ = std::fs::remove_dir("build");
    pkgbuilds.link_pkgs();
    if ! settings.noclean {
        pkgbuilds.clean_pkgdir();
    }
    if r.is_err() {
        Err("Failed to build")
    } else {
        Ok(())
    }
}

fn main() -> Result<(), &'static str> {
    work(prepare()?)
}
