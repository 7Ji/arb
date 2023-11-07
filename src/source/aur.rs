use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AurPackage {
    pub(crate) last_modified: i64,
    pub(crate) name: String
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct AurResult {
    pub(crate) results: Vec<AurPackage>,
}

impl AurResult {
    pub(crate) fn from_pkgs<I, S>(pkgs: I) -> Result<Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
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
        }
        for i in 0..AUR_MAX_TRIES {
            println!("Requesting AUR, try {} of {}", i + 1, AUR_MAX_TRIES);
            println!("Requesting URL '{}'", url);
            let response = match ureq::get(&url).call() {
                Ok(response) => response,
                Err(e) => {
                    eprintln!("Failed to call AUR: {}", e);
                    continue
                },
            };
            match response.into_json() {
                Ok(result) => return Ok(result),
                Err(e) => eprintln!("Failed to parse response: {}", e),
            }
        }
        eprintln!("Failed to get AUR result after all tries");
        Err(())
    }
}