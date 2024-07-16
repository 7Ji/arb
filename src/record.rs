use std::collections::HashMap;

use git2::Oid;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn serialize_oid<S>(oid: &Oid, serializer: S) -> Result<S::Ok, S::Error> 
where 
    S: Serializer 
{
    serde_bytes::serialize(oid.as_bytes(), serializer)
}

fn deserialize_oid<'de, D>(deserializer: D) -> Result<Oid, D::Error>
where 
    D: Deserializer<'de> 
{
    let raw: [u8; 20] = serde_bytes::deserialize(deserializer)?;
    Ok(Oid::from_bytes(&raw).unwrap_or_else(|e|{
        log::error!("Failed to deserialize OID {:?}: {}, assuming full-zero", 
            raw, e);
        Oid::zero()
    }))
}

#[derive(Serialize, Deserialize)]
struct PackageRecords {
    #[serde(serialize_with = "serialize_oid", deserialize_with = "deserialize_oid")]
    last_treeish: Oid,
    last_pkgver: String,
    last_dephash: u64,
    // The count this package was rebuilt with the same treeish ID and pkgver
    rebuild_count: u64,
}

#[derive(Serialize, Deserialize)]
struct PackagesRecords {
    package_records_map: HashMap<String, PackageRecords> // package: record
}

impl PackagesRecords {
    fn should_build(&mut self, package: &str, treeish: &Oid, pkgver: &str, dephash: u64) -> bool {
        let mut should_build = false;
        match self.package_records_map.entry(package.into()) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let entry = entry.get_mut();
                if entry.last_treeish != *treeish {
                    entry.last_treeish = treeish.clone();
                    should_build = true
                }
                if entry.last_pkgver != pkgver {
                    entry.last_pkgver = pkgver.into();
                    should_build = true
                }
                if entry.last_dephash != dephash {
                    entry.last_dephash = dephash;
                    should_build = true
                }
                if should_build {
                    entry.rebuild_count += 1
                }
            },
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(PackageRecords {
                    last_treeish: treeish.clone(),
                    last_pkgver: pkgver.into(),
                    last_dephash: dephash,
                    rebuild_count: 0,
                });
                should_build = true
            },
        }
        should_build
    }
}