use std::{path::{PathBuf, Path}, fs::{File, hard_link, remove_file}, io::{Read, Write}};
use blake2::Blake2b512;
// use generic_array::{ArrayLength, GenericArray};
// use blake2::digest::generic_array::{GenericArray, ArrayLength};
use crc;
// use crypto_common::OutputSizeUser;
use sha1::{Sha1, Digest, digest::{OutputSizeUser, generic_array::GenericArray}};
use sha2::{Sha224, Sha256, Sha384, Sha512};

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
    println!("No length: {}, has length: {}", CKSUM.checksum(input), digest.finalize());
}

fn cksum(file: &mut File) -> u32 {
    let mut digest = CKSUM.digest();
    let mut buffer = vec![0; BUFFER_SIZE];
    let mut size_total = 0;
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("Failed to read file: {}", e);
                panic!("Failed to read file");
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
    digest.finalize()
}

fn md5sum(file: &mut File) -> [u8; 16] {
    let mut context = md5::Context::new();
    let mut buffer = vec![0; BUFFER_SIZE];
    // file.seek(SeekFrom::Start(0)).expect("Failed to seek file");
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("Failed to read file: {}", e);
                panic!("Failed to read file");
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        context.consume(chunk);
    }
    context.compute().0
}

fn generic_sum<T: Digest + OutputSizeUser>(file: &mut File) -> GenericArray<u8, T::OutputSize> {
    let mut hasher = T::new();
    let mut buffer = vec![0; BUFFER_SIZE];
    // file.seek(SeekFrom::Start(0)).expect("Failed to seek file");
    loop {
        let size_chunk = match file.read(&mut buffer) {
            Ok(size) => size,
            Err(e) => {
                eprintln!("Failed to read file: {}", e);
                panic!("Failed to read file");
            },
        };
        if size_chunk == 0 {
            break
        }
        let chunk = &buffer[0..size_chunk];
        hasher.update(chunk);
    }
    hasher.finalize()
}

fn sha1sum(file: &mut File) -> [u8; 20] {
    return generic_sum::<Sha1>(file).into();
}

fn sha224sum(file: &mut File) -> [u8; 28] {
    return generic_sum::<Sha224>(file).into();
}

fn sha256sum(file: &mut File) -> [u8; 32] {
    return generic_sum::<Sha256>(file).into();
}

fn sha384sum(file: &mut File) -> [u8; 48] {
    return generic_sum::<Sha384>(file).into();
}

fn sha512sum(file: &mut File) -> [u8; 64] {
    return generic_sum::<Sha512>(file).into();
}

fn b2sum(file: &mut File) -> [u8; 64] {
    return generic_sum::<Blake2b512>(file).into();
}

pub(crate) fn optional_equal<C:PartialEq>(a: &Option<C>, b: &Option<C>) -> bool {
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

pub(crate) fn get_integ_file(parent: &str, integ: Integ) -> IntegFile {
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
    IntegFile { path: PathBuf::from(format!("{}/{}", parent, name)), integ}
}

pub(crate) fn valid_integ_file(integ_file: &IntegFile, skipint: bool) -> bool {
    if ! integ_file.path.exists() {
        eprintln!("Integ file '{}' does not exist", integ_file.path.display());
        return false
    }
    if skipint {
        eprintln!("Integrity check skipped for existing '{}'", integ_file.path.display());
        return true
    }
    let mut file = match File::open(&integ_file.path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to open file '{}': {}", integ_file.path.display(), e);
            match std::fs::remove_file(&integ_file.path) {
                Ok(_) => (),
                Err(e) => {
                    eprintln!("Failed to remove file '{}' that we could not access correctly: {}", integ_file.path.display(), e);
                    panic!("Failed to remove file we could not open correctly");
                },
            }
            return false
        },
    };
    return match integ_file.integ {
        Integ::CK { ck } => cksum(&mut file) == ck,
        Integ::MD5 { md5 } => md5sum(&mut file) == md5,
        Integ::SHA1 { sha1 } => sha1sum(&mut file) == sha1,
        Integ::SHA224 { sha224 } => sha224sum(&mut file) == sha224,
        Integ::SHA256 { sha256 } => sha256sum(&mut file) == sha256,
        Integ::SHA384 { sha384 } => sha384sum(&mut file) == sha384,
        Integ::SHA512 { sha512 } => sha512sum(&mut file) == sha512,
        Integ::B2 { b2 } => b2sum(&mut file) == b2,
    }
}

pub(crate) fn clone_integ_file(target: &IntegFile, source: &IntegFile) {
    if target.path.exists() {
        match remove_file(&target.path) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Failed to remove file {}: {}", &target.path.display(), e);
                panic!("Failed to remove existing target file");
            },
        }
    }
    match hard_link(&source.path, &target.path) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Failed to link {} to {}: {}", target.path.display(), source.path.display(), e);
            let mut target_file = match File::create(&target.path) {
                Ok(target_file) => target_file,
                Err(e) => {
                    eprintln!("Failed to open {} as write-only: {}", target.path.display(), e);
                    panic!("Failed to open target file as write-only");
                },
            };
            let mut source_file = match File::open(&source.path) {
                Ok(source_file) => source_file,
                Err(e) => {
                    eprintln!("Failed to open {} as read-only: {}", source.path.display(), e);
                    panic!("Failed to open source file as read-only");
                },
            };
            let mut buffer = vec![0; BUFFER_SIZE];
            loop {
                let size_chunk = match source_file.read(&mut buffer) {
                    Ok(size) => size,
                    Err(e) => {
                        eprintln!("Failed to read file: {}", e);
                        panic!("Failed to read file");
                    },
                };
                if size_chunk == 0 {
                    break
                }
                let chunk = &buffer[0..size_chunk];
                match target_file.write_all(chunk) {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Failed to write {} bytes into target file {}: {}", size_chunk, target.path.display(), e);
                        panic!("Failed to write into target file");
                    },
                }
            }
        },
    }
    if ! valid_integ_file(target, false) {
        panic!("Cloned integ file not healthy");
    }
}