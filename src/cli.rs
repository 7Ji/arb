
use std::path::PathBuf;

use crate::{filesystem::action_rm_rf, pkgbuild::action_read_pkgbuilds, rootless::{action_broker, action_init, action_map_assert}, worker::{WorkerStateBuilt, WorkerStateFetchedPkgbuilds, WorkerStateFetchedPkgs, WorkerStateFetchedSources, WorkerStateReadConfig, WorkerStateReleased}, Result};

#[derive(clap::Args, Debug, Clone)]
pub(crate) struct ActionArgs {
    /// Architecture, useful if building for multi-arch, note only setting this
    /// won't make cross-compiling happen magically. If 'auto' or 'any', arb 
    /// would dump the actual arch from `makepkg.conf`
    #[arg(short = 'a', long)]
    pub(crate) arch: Option<String>,

    /// Generate a list of Git repos that could be used by 7Ji/git-mirrorer and 
    /// write it to file, to stdout if set to -
    #[arg(long, default_value_t)]
    pub(crate) gengmr: String,

    /// Prefix of a 7Ji/git-mirrorer instance, e.g. git://gmr.lan,
    /// the mirror would be tried first before actual git remotes
    #[arg(short='g', long)]
    pub(crate) gmr: Option<String>,

    /// Hold versions of git sources, do not update them
    #[arg(short='G', long)]
    pub(crate) holdgit: Option<bool>,

    /// Hold versions of PKGBUILDs, do not update them
    #[arg(short='P', long)]
    pub(crate) holdpkg: Option<bool>,

    /// Skip integrity check for netfile sources if they're found
    #[arg(short='I', long)]
    pub(crate) lazyint: Option<bool>,

    /// Attempt without proxy for this amount of tries before actually using
    /// the proxy
    #[arg(short='X', long)]
    pub(crate) lazyproxy: Option<usize>,

    /// Path to makepkg.conf
    #[arg(short='m', long)]
    pub(crate) mpconf: Option<String>,

    /// Do not actually build the packages after extraction
    #[arg(short='B', long)]
    pub(crate) nobuild: Option<bool>,

    /// Do not clean unused sources and outdated packages
    #[arg(short='C', long)]
    pub(crate) noclean: Option<bool>,

    /// Disallow any network connection during build routine
    #[arg(short='N', long)]
    pub(crate) nonet: Option<bool>,

    /// Path to pacman.conf
    #[arg(long)]
    pub(crate) paconf: Option<String>,

    /// Proxy for git updating and http(s), currently support only http
    #[arg(short, long)]
    pub(crate) proxy: Option<String>,

    /// The GnuPG key ID used to sign packages
    #[arg(short, long)]
    pub(crate) sign: Option<String>,

    /// The path to config file
    #[arg(default_value_t = String::from("config.yaml"))]
    pub(crate) config: String,

    /// Only do action for the specific PKGBUILD(s), for all if none is set
    pub(crate) chosen: Vec<String>,
}

impl ActionArgs {
    fn try_fetch_pkgbuilds(self) -> Result<WorkerStateFetchedPkgbuilds> {
        WorkerStateReadConfig::try_new(&self.config)?
            .try_merge_config(self)?
            .try_prepare_rootless()?
            .try_prepare_layout()?
            .try_fetch_pkgbuilds()
    }

    fn try_fetch_sources(self) -> Result<WorkerStateFetchedSources> {
        self.try_fetch_pkgbuilds()?
            .try_prepare_base_root()?
            .try_dump_arch()?
            .try_parse_pkgbuilds()?
            .try_fetch_sources()
    }

    fn try_fetch_pkgs(self) -> Result<WorkerStateFetchedPkgs> {
        self.try_fetch_sources()?
            .try_fetch_pkgs()
    }

    fn try_build(self) -> Result<WorkerStateBuilt> {
        self.try_fetch_pkgs()?
            .try_build()
    }

    fn try_release(self) -> Result<WorkerStateReleased> {
        self.try_build()?
            .try_release()
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Action {
    /// Fetch PKBUILDs
    FetchPkgbuilds (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then fetch sources
    FetchSources (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then fetch dependent pkgs
    FetchPkgs (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then build PKGBUILDs
    Build (
        #[command(flatten)]
        ActionArgs
    ),
    /// ..., then create release
    Release (
        #[command(flatten)]
        ActionArgs
    ),
    /// Do everything above. End users should only use this instead of the above split actions
    DoEverything (
        #[command(flatten)]
        ActionArgs
    ),
    #[clap(hide = true)]
    MapAssert,
    #[clap(hide = true)]
    ReadPkgbuilds {
        root: PathBuf,
        pkgbuilds: Vec<PathBuf>,
    },
    #[clap(hide = true)]
    RmRf {
        paths: Vec<PathBuf>,
    },
    /// An intermediate stage to spawn later process that's wrapped by init
    #[clap(hide = true)]
    Broker,
    /// Spawn a pseudo init process
    #[clap(hide = true)]
    Init,
}

#[derive(clap::Parser, Debug)]
#[command(version)]
struct Arg {
    #[command(subcommand)]
    action: Action,
}

pub(crate) fn work() -> Result<()> {
    log::debug!("Args: {:?}", std::env::args());
    let arg: Arg = clap::Parser::parse();
    match arg.action {
        Action::FetchPkgbuilds(args) => args.try_fetch_pkgbuilds().and(Ok(())),
        Action::FetchSources(args) => args.try_fetch_sources().and(Ok(())),
        Action::FetchPkgs(args) => args.try_fetch_pkgs().and(Ok(())),
        Action::Build(args) => args.try_build().and(Ok(())),
        Action::Release(args) => args.try_release().and(Ok(())),
        Action::DoEverything(args) => args.try_release().and(Ok(())),
        Action::MapAssert => action_map_assert(),
        Action::ReadPkgbuilds { root, pkgbuilds } => action_read_pkgbuilds(root, &pkgbuilds),
        Action::RmRf { paths } => action_rm_rf(paths),
        Action::Broker => action_broker(),
        Action::Init => action_init(),
    }
}