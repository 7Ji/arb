use serde::Deserialize;

use crate::error::{
        Error,
        Result
    };

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AurPackage {
    pub(crate) last_modified: i64,
    pub(crate) name: String
}

#[derive(Deserialize, Debug, Default)]
pub(crate) struct AurResult {
    pub(crate) results: Vec<AurPackage>,
}

impl AurResult {
    pub(crate) fn len(&self) -> usize {
        self.results.len()
    }

    pub(crate) fn from_pkgs<I, S>(pkgs: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
        // Different from the one we would get from AUR API, this would be in 
        // the same order of pkgs
        let mut result_filled = AurResult::default();
        const AUR_MAX_TRIES: usize = 3;
        let mut url = String::from(
            "https://aur.archlinux.org/rpc/v5/info?");
        let mut started = false;
        for pkg in pkgs {
            if started {
                url.push('&')
            } else {
                started = true
            }
            url.push_str("arg%5B%5D="); // arg[]=
            url.push_str(pkg.as_ref());
            result_filled.results.push(AurPackage {
                last_modified: i64::MAX, name: pkg.as_ref().into()});
        }
        let mut last_error = Error::ImpossibleLogic;
        for i in 0..AUR_MAX_TRIES {
            log::info!("Requesting AUR, try {} of {}", i + 1, AUR_MAX_TRIES);
            log::info!("Requesting URL '{}'", url);
            let response = match ureq::get(&url).call() {
                Ok(response) => response,
                Err(e) => {
                    log::error!("Failed to call AUR: {}", e);
                    last_error = e.into();
                    continue
                },
            };
            match response.into_json() {
                Ok(result) => {
                    result_filled.merge(&result)?;
                    return Ok(result_filled)
                },
                Err(e) => {
                    log::error!("Failed to parse response: {}", e);
                    last_error = e.into()
                },
            }
        }
        log::error!("Failed to get AUR result after all tries");
        Err(last_error)
    }

    /// Merge data in `other` into `self`
    fn merge(&mut self, other: &Self) -> Result<()> {
        // Assumption: if self[i] == other[j], then i >= j, i.e. there could 
        // only be missing object in other from self, but not redundant object
        // in other that did not exist in self.
        
        // If AUR does decide to return in a different order... Well at least
        // that's not my fault.
        let count_self = self.len();
        let count_other = other.len();
        if count_self < count_other {
            log::error!("Cannot merge from a longer result");
            return Err(Error::InvalidArgument)
        }
        let mut index_other = 0;
        for index_self in 0..count_self {
            let result_self = &mut self.results[index_self];
            if index_other < count_other {
                let result_other = &other.results[index_other];
                if result_self.name == result_other.name {
                    result_self.last_modified = result_other.last_modified;
                    index_other += 1;
                }
            } else {
                result_self.last_modified = std::i64::MAX
            }
        }
        Ok(())
    }
}