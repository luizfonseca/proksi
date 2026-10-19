//! The admin UI of Proksi (ADR 0008): the files of its build, embedded, served by the admin
//! listener

/// A file of the UI, with its compressed versions
pub struct Asset {
    /// Its path, from `/`
    pub path: &'static str,
    pub content_type: &'static str,
    pub body: &'static [u8],
    pub brotli: Option<&'static [u8]>,
    pub gzip: Option<&'static [u8]>,
}

impl std::fmt::Debug for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asset")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Files are the same when their paths are
impl PartialEq for Asset {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Asset {
    /// Files under `/assets/` have a hash of their content in their name, so they never change
    #[must_use]
    pub fn immutable(&self) -> bool {
        self.path.starts_with("/assets/")
    }
}

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// Shown when the UI wasn't built
static NOT_BUILT: Asset = Asset {
    path: "/index.html",
    content_type: "text/html; charset=utf-8",
    body: b"<!doctype html><meta charset=utf-8><title>Proksi</title>\
<p>This build of Proksi has no admin UI: build it with <code>npm ci && npm run build</code> \
in <code>crates/proksi-admin-ui</code>, then build Proksi again.</p>",
    brotli: None,
    gzip: None,
};

/// The file at `path`. Paths that aren't files are the UI's own pages, and get `index.html`.
#[must_use]
pub fn get(path: &str) -> Option<&'static Asset> {
    let path = if path == "/" { "/index.html" } else { path };
    if let Some(asset) = ASSETS.iter().find(|asset| asset.path == path) {
        return Some(asset);
    }
    // A missing file (`/assets/old-hash.js`) is a 404, not a page
    if path.starts_with("/assets/") {
        return None;
    }
    ASSETS
        .iter()
        .find(|asset| asset.path == "/index.html")
        .or(Some(&NOT_BUILT))
}

/// Whether the UI was built into this binary
#[must_use]
pub fn built() -> bool {
    !ASSETS.is_empty()
}

#[cfg(test)]
mod tests {
    use super::get;

    #[test]
    fn test_pages_get_the_index() {
        let index = get("/").unwrap();
        assert_eq!(index.path, "/index.html");
        assert_eq!(get("/hosts/example.com").unwrap().path, "/index.html");
        assert!(get("/assets/missing-1234.js").is_none());
    }
}
