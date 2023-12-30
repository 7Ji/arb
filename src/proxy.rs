#[derive(Default, Clone)]
pub(crate) struct Proxy {
    pub(crate) url: String,
    pub(crate) after: usize,
}

pub(crate) const NOPROXY: Proxy = Default::default();

impl Proxy {
    /// Get the amount of tries we should do without/with proxy
    pub(crate) fn get_tries(&self, base: usize) -> (usize, usize) {
        if self.url.is_empty() {
            (base, 0)
        } else {
            (self.after, base)
        }
    }
}