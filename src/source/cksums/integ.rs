use std::{
    fs::File,
    path::{
        PathBuf,
        Path
    },
};

use super::ck::Cksum;
use super::crypto::{
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
    B2sum,
};
use super::md5::Md5sum;
use super::Sum;

pub(crate) enum Integ {
    CK {sum: Cksum},
    MD5 {sum: Md5sum},
    SHA1 {sum: Sha1sum},
    SHA224 {sum: Sha224sum},
    SHA256 {sum: Sha256sum},
    SHA384 {sum: Sha384sum},
    SHA512 {sum: Sha512sum},
    B2 {sum: B2sum}
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

pub(crate) fn string_from(cksum: &[u8]) -> String {
    String::from_iter(cksum.iter().map(|byte| format!("{:02x}", byte)))
}

impl IntegFile {
    pub(crate) fn from_integ(parent: &str, integ: Integ) -> Self {
        let name = match &integ {
            Integ::CK { sum } => sum.to_string(),
            Integ::MD5 { sum } => sum.to_string(),
            Integ::SHA1 { sum } => sum.to_string(),
            Integ::SHA224 { sum } => sum.to_string(),
            Integ::SHA256 { sum } => sum.to_string(),
            Integ::SHA384 { sum } => sum.to_string(),
            Integ::SHA512 { sum } => sum.to_string(),
            Integ::B2 { sum } => sum.to_string(),
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
            Ok(mut file) => {
                let file = &mut file;
                match self.integ {
                    Integ::CK { sum } => 
                        Cksum::sum(file) == Some(sum),
                    Integ::MD5 { sum } => 
                        Md5sum::sum(file) == Some(sum),
                    Integ::SHA1 { sum } => 
                        Sha1sum::sum(file) == Some(sum),
                    Integ::SHA224 { sum } => 
                        Sha224sum::sum(file) == Some(sum),
                    Integ::SHA256 { sum } => 
                        Sha256sum::sum(file) == Some(sum),
                    Integ::SHA384 { sum } => 
                        Sha384sum::sum(file) == Some(sum),
                    Integ::SHA512 { sum } => 
                        Sha512sum::sum(file) == Some(sum),
                    Integ::B2 { sum } => 
                        B2sum::sum(file) == Some(sum),
                }
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
        if let Err(e) = super::super::download::clone_file(
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