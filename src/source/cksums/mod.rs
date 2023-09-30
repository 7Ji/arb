mod ck;
mod crypto;
mod integ;
mod md5;

const BUFFER_SIZE: usize = 0x400000; // 4M

pub(super) use ck::Cksum;
pub(super) use crypto::{
    B2sum,
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
};
pub(super) use md5::Md5sum;
pub(super) use integ::IntegFile;


pub(super) trait Sum {
    fn sum(file: &mut std::fs::File) -> Option<Self> where Self: Sized;
    fn from_hex(hex: &[u8]) -> Option<Self> where Self: Sized;
}