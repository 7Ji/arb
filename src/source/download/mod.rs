mod common;
mod file;
mod ftp;
mod http;
mod rsync;
mod scp;

pub(crate) use file::{
    clone_file,
    file
};
pub(crate) use ftp::ftp;
pub(crate) use http::http;
pub(crate) use rsync::rsync;
pub(crate) use scp::scp;