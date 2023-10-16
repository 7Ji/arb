#[derive(Clone)]
pub(crate) struct Proxy {
    pub(super) url: String,
    pub(super) after: usize,
}


impl Proxy {
    pub(crate) fn from_str_usize(url: Option<&str>, after: usize) -> Option<Self> {
        match url {
            Some(url) => Some(Self { url: url.to_string(), after}),
            None => None,
        }
    }
}

