use blake2::Blake2b512;
use crc;
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
        path::{
            PathBuf,
            Path
        },
    };

use crate::download;

pub(crate) enum Integ {
    CK {ck: u32},
    MD5 {md5: [u8; 16]},
    SHA1 {sha1: [u8; 20]},
    SHA224 {sha224: [u8; 28]},
    SHA256 {sha256: [u8; 32]},
    SHA384 {sha384: [u8; 48]},
    SHA512 {sha512: [u8; 64]},
    B2 {b2: [u8; 64]}
}

pub(crate) struct IntegFile {
    path: PathBuf,
    integ: Integ
}

impl IntegFile {
    pub(crate) fn get_path(&self) -> &Path {
        self.path.as_path()
    }
}

const BUFFER_SIZE: usize = 0x400000; // 4M

const CKSUM: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_CKSUM);

fn _cksum(input: &[u8]) {
    let mut digest = CKSUM.digest();
    digest.update(input);
    let mut len_oct = Vec::<u8>::new();
    let mut len = input.len();
    if len > 0 {
        while len > 0 {
            len_oct.push((len & 0xFF).try_into().unwrap());
            len >>= 8;
        }
    } else {
        len_oct.push(0);
    }
    digest.update(&len_oct);
    println!("No length: {}, has length: {}",
                CKSUM.checksum(input), digest.finalize());
}

fn cksum(file: &mut File) -> Option<u32> {
    let mut digest = CKSUM.digest();
    let mut buffer = vec![0; BUFFER_SIZE];
    let mut size_total = 0;
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
    Some(digest.finalize())
}

fn md5sum(file: &mut File) -> Option<[u8; 16]> {
    let mut context = md5::Context::new();
    let mut buffer = vec![0; BUFFER_SIZE];
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
        context.consume(chunk);
    }
    Some(context.compute().0)
}

fn generic_sum<T: Digest + OutputSizeUser>(file: &mut File)
    -> Option<GenericArray<u8, T::OutputSize>>
{
    let mut hasher = T::new();
    let mut buffer = vec![0; BUFFER_SIZE];
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

fn sha1sum(file: &mut File) -> Option<[u8; 20]> {
    generic_sum::<Sha1>(file)
        .map(|sum|sum.into())
}

fn sha224sum(file: &mut File) -> Option<[u8; 28]> {
    generic_sum::<Sha224>(file)
        .map(|sum|sum.into())
}

fn sha256sum(file: &mut File) -> Option<[u8; 32]> {
    generic_sum::<Sha256>(file)
        .map(|sum|sum.into())
}

fn sha384sum(file: &mut File) -> Option<[u8; 48]> {
    generic_sum::<Sha384>(file)
        .map(|sum|sum.into())
}

fn sha512sum(file: &mut File) -> Option<[u8; 64]> {
    generic_sum::<Sha512>(file)
        .map(|sum|sum.into())
}

fn b2sum(file: &mut File) -> Option<[u8; 64]> {
    generic_sum::<Blake2b512>(file)
        .map(|sum|sum.into())
}

pub(crate) fn optional_equal<C:PartialEq>(a: &Option<C>, b: &Option<C>)
    -> bool
{
    if let Some(a) = a {
        if let Some(b) = b {
            if a == b {
                return true
            }
        }
    }
    false
}

pub(crate) fn optional_update<C>(target: &mut Option<C>, source: &Option<C>)
where C: PartialEq + Clone {
    if let Some(target) = target {
        if let Some(source) = source {
            if target == source {
                return;
            } else {
                panic!("Source target mismatch");
            }
        } else {
            return;
        }
    } else if let Some(source) = source {
        *target = Some(source.clone())
    }
}

pub(crate) fn string_from(cksum: &[u8]) -> String {
    String::from_iter(cksum.iter().map(|byte| format!("{:02x}", byte)))
}

impl IntegFile {
    pub(crate) fn from_integ(parent: &str, integ: Integ) -> Self {
        let name = match &integ {
            Integ::CK { ck } => format!("{:08x}", ck),
            Integ::MD5 { md5 } => string_from(md5),
            Integ::SHA1 { sha1 } => string_from(sha1),
            Integ::SHA224 { sha224 } => string_from(sha224),
            Integ::SHA256 { sha256 } => string_from(sha256),
            Integ::SHA384 { sha384 } => string_from(sha384),
            Integ::SHA512 { sha512 } => string_from(sha512),
            Integ::B2 { b2 } => string_from(b2),
        };
        Self { path: PathBuf::from(format!("{}/{}", parent, name)), integ}
    }

    pub(crate) fn valid(&self, skipint: bool) -> bool {
        if ! self.path.exists() {
            eprintln!("Integ file '{}' does not exist", self.path.display());
            return false
        }
        if skipint {
            eprintln!("Integrity check skipped for existing '{}'",
                        self.path.display());
            return true
        }
        let valid = match File::open(&self.path) {
            Ok(mut file) => match self.integ {
                Integ::CK { ck } =>
                    cksum(&mut file) == Some(ck),
                Integ::MD5 { md5 } =>
                    md5sum(&mut file) == Some(md5),
                Integ::SHA1 { sha1 } =>
                    sha1sum(&mut file) == Some(sha1),
                Integ::SHA224 { sha224 } =>
                    sha224sum(&mut file) == Some(sha224),
                Integ::SHA256 { sha256 } =>
                    sha256sum(&mut file) == Some(sha256),
                Integ::SHA384 { sha384 } =>
                    sha384sum(&mut file) == Some(sha384),
                Integ::SHA512 { sha512 } =>
                    sha512sum(&mut file) == Some(sha512),
                Integ::B2 { b2 } =>
                    b2sum(&mut file) == Some(b2),
            },
            Err(e) => {
                eprintln!("Failed to open file '{}': {}",
                            self.path.display(), e);
                false
            },
        };
        if ! valid {
            match std::fs::remove_file(&self.path) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!(
                        "Failed to remove bad file '{}': {}",
                              self.path.display(), e);
                    panic!("Failed to remove bad file");
                },
            }
        }
        return valid;
    }

    pub(crate) fn clone_file_from(&self, source: &Self) -> Result<(), ()> {
        if let Err(e) = download::clone_file(
            &source.path, &self.path) 
        {
            eprintln!("Failed to clone '{}' from '{}': {}", 
                        self.path.display(),
                        source.path.display(),
                        e);
            return Err(())
        }
        if self.valid(false) {
            Ok(())
        } else {
            eprintln!("Cloned integ file not healthy");
            Err(())
        }
    }
}