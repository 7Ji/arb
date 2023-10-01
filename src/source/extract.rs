use std::{
        os::unix::fs::symlink,
        path::{
            Path,
            PathBuf,
        }
    };

use super::{
    VcsProtocol,
    Protocol,
    Source,
    IntegFile,
};

use xxhash_rust::xxh3::xxh3_64;

pub(crate) fn extract<P: AsRef<Path>>(dir: P, sources: &Vec<Source>) {
    let rel = PathBuf::from("../..");
    for source in sources.iter() {
        let mut original = None;
        match &source.protocol {
            Protocol::Netfile { protocol: _ } => {
                let integ_files = IntegFile::vec_from_source(source);
                if let Some(integ_file) = integ_files.last() {
                    original = Some(rel.join(integ_file.get_path()));
                }
            },
            Protocol::Vcs { protocol } =>
                if let VcsProtocol::Git = protocol {
                    original = Some(rel
                        .join(format!("sources/git/{:016x}",
                                xxh3_64(source.url.as_bytes()))));
                },
            Protocol::Local => (),
        }
        if let Some(original) = original {
            symlink(original,
                dir.as_ref().join(&source.name))
                .expect("Failed to symlink")
        }
    }
}