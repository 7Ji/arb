use blake2::Blake2b512;
use sha1::{
        Digest,
        digest::{
            OutputSizeUser,
            generic_array::GenericArray,
        },
        Sha1,
    };
use sha2::{
        Sha224,
        Sha256,
        Sha384,
        Sha512,
    };
use std::{
        fs::File,
        io::Read,
    };

fn generic_sum<T: Digest + OutputSizeUser>(file: &mut File)
    -> Option<GenericArray<u8, T::OutputSize>>
{
    let mut hasher = T::new();
    let mut buffer = vec![0; super::BUFFER_SIZE];
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("Failed to read file: {}", e);
                return None
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        hasher.update(chunk);
    }
    Some(hasher.finalize())
}

pub(super) fn sha1sum(file: &mut File) -> Option<[u8; 20]> {
    generic_sum::<Sha1>(file)
        .map(|sum|sum.into())
}

pub(super) fn sha224sum(file: &mut File) -> Option<[u8; 28]> {
    generic_sum::<Sha224>(file)
        .map(|sum|sum.into())
}

pub(super) fn sha256sum(file: &mut File) -> Option<[u8; 32]> {
    generic_sum::<Sha256>(file)
        .map(|sum|sum.into())
}

pub(super) fn sha384sum(file: &mut File) -> Option<[u8; 48]> {
    generic_sum::<Sha384>(file)
        .map(|sum|sum.into())
}

pub(super) fn sha512sum(file: &mut File) -> Option<[u8; 64]> {
    generic_sum::<Sha512>(file)
        .map(|sum|sum.into())
}

pub(super) fn b2sum(file: &mut File) -> Option<[u8; 64]> {
    generic_sum::<Blake2b512>(file)
        .map(|sum|sum.into())
}