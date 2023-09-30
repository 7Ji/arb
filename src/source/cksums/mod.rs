use std::{
        fs::File,
        path::{
            PathBuf,
            Path
        },
    };

mod ck;
mod crypto;
mod md5;

use ck::cksum;
use crypto::{
    sha1sum,
    sha224sum,
    sha256sum,
    sha384sum,
    sha512sum,
    b2sum
};
use md5::md5sum;

const BUFFER_SIZE: usize = 0x400000; // 4M

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
-> Result<(), ()>
    where C: PartialEq + Clone 
{
    if let Some(target) = target {
        if let Some(source) = source {
            if target != source {
                eprintln!("Source target mismatch");
                return Err(());
            }
        }
    } else if let Some(source) = source {
        *target = Some(source.clone())
    }
    Ok(())
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
                },
            }
        }
        return valid;
    }

    pub(crate) fn clone_file_from(&self, source: &Self) -> Result<(), ()> {
        if let Err(e) = super::download::clone_file(
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