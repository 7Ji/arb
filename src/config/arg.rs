use clap::Parser;


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Arg {
    /// Optional config.yaml file
    #[arg(default_value_t = String::from("config.yaml"))]
    pub(crate) config: String,

    /// Optional packages to only build them
    pub(crate) pkgs: Vec<String>,

    /// HTTP proxy to retry for git updating and http(s)
    /// netfiles if attempt without proxy failed
    #[arg(short, long)]
    pub(crate) proxy: Option<String>,

    /// Attempt without proxy for this amount of tries before actually using
    /// the proxy, to save bandwidth
    #[arg(long)]
    pub(crate) proxy_after: Option<usize>,

    /// Hold versions of PKGBUILDs, do not update them
    #[arg(short='P', long, default_value_t = false)]
    pub(crate) holdpkg: bool,

    /// Hold versions of git sources, do not update them
    #[arg(short='G', long, default_value_t = false)]
    pub(crate) holdgit: bool,

    /// Skip integrity check for netfile sources if they're found
    #[arg(short='I', long, default_value_t = false)]
    pub(crate) skipint: bool,

    /// Do not actually build the packages
    #[arg(short='B', long, default_value_t = false)]
    pub(crate) nobuild: bool,

    /// Do not clean unused sources and outdated packages
    #[arg(short='C', long, default_value_t = false)]
    pub(crate) noclean: bool,

    /// Disallow any network connection during makepkg's build routine
    #[arg(short='N', long, default_value_t = false)]
    pub(crate) nonet: bool,

    /// Drop to the specific uid:gid pair, instead of getting from SUDO_UID/GID
    #[arg(short='d', long)]
    pub(crate) drop: Option<String>,

    /// Prefix of a 7Ji/git-mirrorer instance, e.g. git://gmr.lan,
    /// The mirror would be tried first before actual git remote
    #[arg(short='g', long)]
    pub(crate) gmr: Option<String>,

    /// The GnuPG key ID used to sign packages
    #[arg(short, long)]
    pub(crate) sign: Option<String>
}