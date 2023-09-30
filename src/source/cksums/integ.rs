use std::{
    fs::File,
    path::{
        PathBuf,
        Path
    },
};

use super::ck::Cksum;
use super::crypto::{
    B2sum,
    Sha1sum,
    Sha224sum,
    Sha256sum,
    Sha384sum,
    Sha512sum,
};
use super::md5::Md5sum;
use super::Sum;

pub(crate) enum Integ {
    CK (Cksum),
    MD5 (Md5sum),
    SHA1 (Sha1sum),
    SHA224 (Sha224sum),
    SHA256 (Sha256sum),
    SHA384 (Sha384sum),
    SHA512 (Sha512sum),
    B2 (B2sum),
}

pub(crate) struct IntegFile {
    path: PathBuf,
    integ: Integ
}

impl IntegFile {
    pub(crate) fn get_path(&self) -> &Path {
        self.path.as_path()
    }
    
    pub(crate) fn from_integ(parent: &str, integ: Integ) -> Self {
        let name = match &integ {
            Integ::CK ( sum ) => sum.to_string(),
            Integ::MD5 ( sum ) => sum.to_string(),
            Integ::SHA1 ( sum ) => sum.to_string(),
            Integ::SHA224 ( sum ) => sum.to_string(),
            Integ::SHA256 ( sum ) => sum.to_string(),
            Integ::SHA384 ( sum ) => sum.to_string(),
            Integ::SHA512 ( sum ) => sum.to_string(),
            Integ::B2 ( sum ) => sum.to_string(),
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
                match &self.integ {
                    Integ::CK ( sum ) => 
                        Cksum::sum(file).as_ref() == Some(sum),
                    Integ::MD5 ( sum ) => 
                        Md5sum::sum(file).as_ref() == Some(sum),
                    Integ::SHA1 ( sum ) => 
                        Sha1sum::sum(file).as_ref() == Some(sum),
                    Integ::SHA224 ( sum ) => 
                        Sha224sum::sum(file).as_ref() == Some(sum),
                    Integ::SHA256 ( sum ) => 
                        Sha256sum::sum(file).as_ref() == Some(sum),
                    Integ::SHA384 ( sum ) => 
                        Sha384sum::sum(file).as_ref() == Some(sum),
                    Integ::SHA512 ( sum ) => 
                        Sha512sum::sum(file).as_ref() == Some(sum),
                    Integ::B2 ( sum ) => 
                        B2sum::sum(file).as_ref() == Some(sum),
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

    pub(crate) fn vec_from_source(source: &super::super::Source) -> Vec<Self> {
        let mut integ_files = vec![];
        if let Some(sum) = &source.ck {
            integ_files.push(Self::from_integ(
                "sources/file-ck", Integ::CK ( sum.clone() )))
        }
        if let Some(sum) = &source.md5 {
            integ_files.push(Self::from_integ(
                "sources/file-md5", Integ::MD5 ( sum.clone() )))
        }
        if let Some(sum) = &source.sha1 {
            integ_files.push(Self::from_integ
                ("sources/file-sha1", Integ::SHA1 ( sum.clone() )))
        }
        if let Some(sum) = &source.sha224 {
            integ_files.push(Self::from_integ(
                "sources/file-sha224", Integ::SHA224 ( sum.clone() )))
        }
        if let Some(sum) = &source.sha256 {
            integ_files.push(Self::from_integ(
                "sources/file-sha256", Integ::SHA256 ( sum.clone() )))
        }
        if let Some(sum) = &source.sha384 {
            integ_files.push(Self::from_integ(
                "sources/file-sha384", Integ::SHA384 ( sum.clone() )))
        }
        if let Some(sum) = &source.sha512 {
            integ_files.push(Self::from_integ(
                "sources/file-sha512", Integ::SHA512 ( sum.clone() )))
        }
        if let Some(sum) = &source.b2 {
            integ_files.push(Self::from_integ(
                "sources/file-b2", Integ::B2 ( sum.clone() )))
        }
        integ_files

    }
}