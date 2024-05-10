use std::{fs::File, io::Read, path::Path};

use pkgbuild::{B2sum, Cksum, Md5sum, Sha1sum, Sha224sum, Sha256sum, Sha384sum, Sha512sum};

use crate::{Error, Result};

#[derive(PartialEq)]
pub(crate) enum Checksum {
    B2sum (B2sum),
    Sha512sum (Sha512sum),
    Sha384sum (Sha384sum),
    Sha256sum (Sha256sum),
    Sha224sum (Sha224sum),
    Sha1sum (Sha1sum),
    Md5sum (Md5sum),
    Cksum (Cksum)
}

const BUFFER_SIZE: usize = 0x100000;

pub(crate) fn crypto_sum_file<T>(file: &mut File) 
-> Result<sha1::digest::generic_array::GenericArray<u8, T::OutputSize>>
where
    T: sha1::Digest + sha1::digest::OutputSizeUser
{
    let mut hasher = T::new();
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                log::error!("Failed to read file: {}", e);
                return Err(e.into())
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        hasher.update(chunk);
    }
    Ok(hasher.finalize())
}

pub(crate) fn b2sum_file(file: &mut File) -> Result<B2sum> {
    Ok(crypto_sum_file::<blake2::Blake2b512>(file)?.into())
}

pub(crate) fn sha512sum_file(file: &mut File) -> Result<Sha512sum> {
    Ok(crypto_sum_file::<sha2::Sha512>(file)?.into())
}

pub(crate) fn sha384sum_file(file: &mut File) -> Result<Sha384sum> {
    Ok(crypto_sum_file::<sha2::Sha384>(file)?.into())
}

pub(crate) fn sha256sum_file(file: &mut File) -> Result<Sha256sum> {
    Ok(crypto_sum_file::<sha2::Sha256>(file)?.into())
}

pub(crate) fn sha224sum_file(file: &mut File) -> Result<Sha224sum> {
    Ok(crypto_sum_file::<sha2::Sha224>(file)?.into())
}

pub(crate) fn sha1sum_file(file: &mut File) -> Result<Sha1sum> {
    Ok(crypto_sum_file::<sha1::Sha1>(file)?.into())
}

pub(crate) fn md5sum_file(file: &mut File) -> Result<Md5sum> {
    let mut context = md5::Context::new();
    let mut buffer = vec![0; BUFFER_SIZE];
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                log::error!("Failed to read file: {}", e);
                return Err(e.into())
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        context.consume(chunk);
    }
    Ok(context.compute().0)
}

pub(crate) fn cksum_file(file: &mut File) -> Result<Cksum> {
    const CKSUM: crc::Crc<u32> = 
        crc::Crc::<u32>::new(&crc::CRC_32_CKSUM);
    let mut digest = CKSUM.digest();
    let mut buffer = vec![0; BUFFER_SIZE];
    let mut size_total = 0;
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                log::error!("Failed to read file: {}", e);
                return Err(e.into())
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        digest.update(chunk);
        size_total += size_chunk;
    }
    let mut size_oct = Vec::<u8>::new();
    if size_total > 0 {
        while size_total > 0 {
            size_oct.push((size_total & 0xFF).try_into().unwrap());
            size_total >>= 8;
        }
    } else {
        size_oct.push(0);
    }
    digest.update(&size_oct);
    Ok(digest.finalize())
}

impl Checksum {
    pub(crate) fn another_try_from_file<P: AsRef<Path>>(&self, path: P) 
        -> Result<Self> 
    {
        let mut file = File::open(path)?;
        let file = &mut file;
        Ok(match self {
            Checksum::B2sum(_) => Self::B2sum(b2sum_file(file)?),
            Checksum::Sha512sum(_) => Self::Sha512sum(sha512sum_file(file)?),
            Checksum::Sha384sum(_) => Self::Sha384sum(sha384sum_file(file)?),
            Checksum::Sha256sum(_) => Self::Sha256sum(sha256sum_file(file)?),
            Checksum::Sha224sum(_) => Self::Sha224sum(sha224sum_file(file)?),
            Checksum::Sha1sum(_) => Self::Sha1sum(sha1sum_file(file)?),
            Checksum::Md5sum(_) => Self::Md5sum(md5sum_file(file)?),
            Checksum::Cksum(_) => Self::Cksum(cksum_file(file)?),
        })
    }

    pub(crate) fn verify_file<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        Ok(self.another_try_from_file(path)? == *self)
    }
}
