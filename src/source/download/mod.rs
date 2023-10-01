mod child;
mod file;
mod ftp;
mod http;
mod rsync;
mod scp;

const BUFFER_SIZE: usize = 0x400000; // 4M

pub(crate) use file::{
    clone_file,
    file
};
pub(crate) use ftp::ftp;
pub(crate) use http::http_native as http;
pub(crate) use rsync::rsync;
pub(crate) use scp::scp;