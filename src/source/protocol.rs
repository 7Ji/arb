
#[derive(Debug, Clone)]
pub(super) enum NetfileProtocol {
    File,
    Ftp,
    Http,
    Https,
    Rsync,
    Scp,
}

#[derive(Debug, Clone)]
pub(super) enum VcsProtocol {
    Bzr,
    Fossil,
    Git,
    Hg,
    Svn,
}

#[derive(Debug, Clone)]
pub(super) enum Protocol {
    Netfile {
        protocol: NetfileProtocol
    },
    Vcs {
        protocol: VcsProtocol
    },
    Local
}

impl Protocol {
    fn _from_string(value: &str) -> Option<Self> {
        let protocol = match value {
            "file" => Self::Netfile { protocol: NetfileProtocol::File },
            "ftp" => Self::Netfile { protocol: NetfileProtocol::Ftp },
            "http" => Self::Netfile { protocol: NetfileProtocol::Http },
            "https" => Self::Netfile { protocol: NetfileProtocol::Https },
            "rsync" => Self::Netfile { protocol: NetfileProtocol::Rsync },
            "scp" => Self::Netfile { protocol: NetfileProtocol::Scp },
            "bzr" => Self::Vcs { protocol: VcsProtocol::Bzr },
            "fossil" => Self::Vcs { protocol: VcsProtocol::Fossil },
            "git" => Self::Vcs { protocol: VcsProtocol::Git },
            "hg" => Self::Vcs { protocol: VcsProtocol::Hg },
            "svn" => Self::Vcs { protocol: VcsProtocol::Svn },
            "local" => Self::Local,
            &_ => {
                log::error!("Unknown protocol {}", value);
                return None
            },
        };
        Some(protocol)
    }
    pub(super) fn from_raw_string(value: &[u8]) -> Option<Self> {
        let protocol = match value {
            b"file" => Self::Netfile { protocol: NetfileProtocol::File },
            b"ftp" => Self::Netfile { protocol: NetfileProtocol::Ftp },
            b"http" => Self::Netfile { protocol: NetfileProtocol::Http },
            b"https" => Self::Netfile { protocol: NetfileProtocol::Https },
            b"rsync" => Self::Netfile { protocol: NetfileProtocol::Rsync },
            b"scp" => Self::Netfile { protocol: NetfileProtocol::Scp },
            b"bzr" => Self::Vcs { protocol: VcsProtocol::Bzr },
            b"fossil" => Self::Vcs { protocol: VcsProtocol::Fossil },
            b"git" => Self::Vcs { protocol: VcsProtocol::Git },
            b"hg" => Self::Vcs { protocol: VcsProtocol::Hg },
            b"svn" => Self::Vcs { protocol: VcsProtocol::Svn },
            b"local" => Self::Local,
            &_ => {
                log::error!("Unknown protocol {}",
                    String::from_utf8_lossy(value));
                return None
            },
        };
        Some(protocol)
    }
}