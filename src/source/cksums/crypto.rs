use blake2::Blake2b512;
use hex::FromHex;
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

#[derive(PartialEq, Clone)]
pub(crate) struct Sha1sum ([u8; 20]);
#[derive(PartialEq, Clone)]
pub(crate) struct Sha224sum ([u8; 28]);
#[derive(PartialEq, Clone)]
pub(crate) struct Sha256sum ([u8; 32]);
#[derive(PartialEq, Clone)]
pub(crate) struct Sha384sum ([u8; 48]);
#[derive(PartialEq, Clone)]
pub(crate) struct Sha512sum ([u8; 64]);
#[derive(PartialEq, Clone)]
pub(crate) struct B2sum ([u8; 64]);


fn generic_sum<T: Digest + OutputSizeUser>(file: &mut File)
    -> Option<GenericArray<u8, T::OutputSize>>
{
    let mut hasher = T::new();
    let mut buffer = vec![0; super::BUFFER_SIZE];
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                log::error!("Failed to read file: {}", e);
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

impl super::Sum for Sha1sum {
    fn sum(file: &mut std::fs::File) -> Option<Self> {
        generic_sum::<Sha1>(file)
        .map(|sum|Self(sum.into()))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

impl super::Sum for Sha224sum {
    fn sum(file: &mut File) -> Option<Self> {
        generic_sum::<Sha224>(file)
        .map(|sum|Self(sum.into()))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

impl super::Sum for Sha256sum {
    fn sum(file: &mut File) -> Option<Self> {
        generic_sum::<Sha256>(file)
            .map(|sum|Self(sum.into()))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

impl super::Sum for Sha384sum {
    fn sum(file: &mut File) -> Option<Self> {
        generic_sum::<Sha384>(file)
            .map(|sum|Self(sum.into()))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

impl super::Sum for Sha512sum {
    fn sum(file: &mut File) -> Option<Self> {
        generic_sum::<Sha512>(file)
            .map(|sum|Self(sum.into()))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

impl super::Sum for B2sum {
    fn sum(file: &mut File) -> Option<Self> {
        generic_sum::<Blake2b512>(file)
            .map(|sum|Self(sum.into()))
    }

    fn from_hex(hex: &[u8]) -> Option<Self> {
        Some(Self(FromHex::from_hex(hex).ok()?))
    }
}

fn generic_fmt(sum: &[u8], f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for byte in sum {
        if let Err(e) = f.write_fmt(format_args!("{:02x}", byte)) {
            return Err(e);
        }
    }
    Ok(())
}

impl std::fmt::Display for Sha1sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        generic_fmt(&self.0, f)
    }
}

impl std::fmt::Display for Sha224sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        generic_fmt(&self.0, f)
    }
}

impl std::fmt::Display for Sha256sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        generic_fmt(&self.0, f)
    }
}

impl std::fmt::Display for Sha384sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        generic_fmt(&self.0, f)
    }
}

impl std::fmt::Display for Sha512sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        generic_fmt(&self.0, f)
    }
}

impl std::fmt::Display for B2sum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        generic_fmt(&self.0, f)
    }
}