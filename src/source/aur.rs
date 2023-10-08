use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AurPackage {
    pub(crate) last_modified: u64,
    pub(crate) name: String
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct AurResult {
    pub(crate) results: Vec<AurPackage>,
}

fn request_url(url: &str) -> Result<String, ()> {
    println!("Requesting URL '{}'", url);
    let future = async {
        let response = reqwest::get(url).await.or_else(|e|{
            eprintln!("Failed to get response: {}", e);
            Err(())
        })?;
        if response.status() != reqwest::StatusCode::OK {
            eprintln!("Failed to get response");
            return Err(())
        }
        response.text().await.or_else(|e|{
            eprintln!("Failed to get the response body as text: {}", e);
            Err(())
        })
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .or_else(|e|{
            eprintln!("Failed to build async runner: {}", e);
            Err(())
        })?
        .block_on(future)
}

impl AurResult {
    pub(crate) fn from_pkgs<I, S>(pkgs: I) -> Result<Self, ()> 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>
    {
        const AUR_MAX_TRIES: usize = 3;
        let mut url = String::from(
            "https://aur.archlinux.org/rpc/v5/info");
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
            let string = match request_url(&url) {
                Ok(string) => string,
                Err(_) => continue,
            };
            match serde_json::from_str(&string) {
                Ok(result) => return Ok(result),
                Err(e) => eprintln!("Failed to parse result: {}", e),
            }
        }
        eprintln!("Failed to get AUR result after all tries");
        Err(())
    }
}