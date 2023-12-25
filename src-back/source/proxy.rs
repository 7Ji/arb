#[derive(Clone)]
pub(crate) struct Proxy {
    pub(super) url: String,
    pub(super) after: usize,
}

impl Proxy {
    pub(crate) fn new(url: &str, after: usize) -> Self {
        Self { url: url.to_string(), after }
    }
}

