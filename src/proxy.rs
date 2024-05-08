#[derive(Default, Clone)]
pub(crate) struct Proxy {
    url: String,
    after: usize,
}

pub(crate) const NOPROXY: Proxy = Proxy { url: String::new(), after: 0 };

impl Proxy {
    pub(crate) fn get_url(&self) -> &str {
        &self.url
    }
    
    pub(crate) fn from_url_and_after(url: String, after: usize) -> Self {
        Self {
            url,
            after,
        }
    }

    pub(crate) fn tries_without_and_with(&self, base: usize) -> (usize, usize) {
        if self.url.is_empty() {
            (base, 0)
        } else {
            (self.after, base)
        }
    }

    pub(crate) fn _tries_without(&self, base: usize) -> usize {
        if self.url.is_empty() {
            base
        } else {
            self.after
        }
    }

    pub(crate) fn _tries_with(&self, base: usize) -> usize {
        if self.url.is_empty() {
            0
        } else {
            base
        }
    }
}