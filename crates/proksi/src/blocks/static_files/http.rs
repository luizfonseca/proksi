//! What a file's response says about it: its type, its validators (`ETag`, `Last-Modified`),
//! and the part of it asked for (`Range`)

use std::time::{SystemTime, UNIX_EPOCH};

/// The type of a file, by its extension
pub fn content_type(path: &std::path::Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("gz") => "application/gzip",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// A file's validators: its `ETag` (from its size and modification time) and `Last-Modified`
pub struct Validators {
    pub etag: String,
    pub last_modified: String,
    modified_secs: u64,
}

impl Validators {
    pub fn new(len: u64, modified: SystemTime) -> Self {
        let modified_secs = modified
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default();
        Self {
            etag: format!("\"{len:x}-{modified_secs:x}\""),
            last_modified: httpdate::fmt_http_date(modified),
            modified_secs,
        }
    }

    /// Whether the client's copy is still the file: `If-None-Match` (when sent), else
    /// `If-Modified-Since`
    pub fn not_modified(
        &self,
        if_none_match: Option<&str>,
        if_modified_since: Option<&str>,
    ) -> bool {
        if let Some(tags) = if_none_match {
            return tags
                .split(',')
                .map(str::trim)
                .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == self.etag);
        }
        if_modified_since
            .and_then(|date| httpdate::parse_http_date(date).ok())
            .and_then(|date| date.duration_since(UNIX_EPOCH).ok())
            .is_some_and(|since| self.modified_secs <= since.as_secs())
    }
}

/// The part of a file of `len` bytes a `Range` header asks for
#[derive(Debug, PartialEq)]
pub enum Part {
    /// No range (or one this doesn't serve, such as several ranges): the whole file
    Whole,
    /// Bytes `start..=end`
    Range { start: u64, end: u64 },
    /// A range outside the file (`416`)
    Unsatisfiable,
}

pub fn part(range: Option<&str>, len: u64) -> Part {
    let Some(spec) = range.and_then(|range| range.trim().strip_prefix("bytes=")) else {
        return Part::Whole;
    };
    if spec.contains(',') {
        return Part::Whole;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return Part::Whole;
    };
    let (start, end) = (start.trim(), end.trim());
    let (start, end) = match (start.parse::<u64>(), end.parse::<u64>()) {
        // The last `end` bytes
        (Err(_), Ok(suffix)) if start.is_empty() => {
            if suffix == 0 || len == 0 {
                return Part::Unsatisfiable;
            }
            (len.saturating_sub(suffix), len - 1)
        }
        (Ok(start), Err(_)) if end.is_empty() => (start, len.saturating_sub(1)),
        (Ok(start), Ok(end)) if start <= end => (start, end.min(len.saturating_sub(1))),
        _ => return Part::Whole,
    };
    if start >= len {
        return Part::Unsatisfiable;
    }
    Part::Range { start, end }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::{content_type, part, Part, Validators};

    #[test]
    fn test_content_types() {
        assert_eq!(
            content_type("a/app.JS".as_ref()),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type("index.html".as_ref()),
            "text/html; charset=utf-8"
        );
        assert_eq!(content_type("font.woff2".as_ref()), "font/woff2");
        assert_eq!(content_type("README".as_ref()), "application/octet-stream");
    }

    #[test]
    fn test_validators() {
        let modified = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let validators = Validators::new(1234, modified);
        assert_eq!(validators.etag, "\"4d2-6553f100\"");
        assert_eq!(validators.last_modified, "Tue, 14 Nov 2023 22:13:20 GMT");

        assert!(validators.not_modified(Some("\"4d2-6553f100\""), None));
        assert!(validators.not_modified(Some("\"x\", W/\"4d2-6553f100\""), None));
        assert!(!validators.not_modified(Some("\"x\""), Some("Tue, 14 Nov 2023 22:13:20 GMT")));
        assert!(validators.not_modified(None, Some("Tue, 14 Nov 2023 22:13:20 GMT")));
        assert!(!validators.not_modified(None, Some("Tue, 14 Nov 2023 22:13:19 GMT")));
        assert!(!validators.not_modified(None, Some("yesterday")));
        assert!(!validators.not_modified(None, None));
    }

    #[test]
    fn test_ranges() {
        assert_eq!(part(None, 100), Part::Whole);
        assert_eq!(
            part(Some("bytes=0-9"), 100),
            Part::Range { start: 0, end: 9 }
        );
        assert_eq!(
            part(Some("bytes=90-"), 100),
            Part::Range { start: 90, end: 99 }
        );
        assert_eq!(
            part(Some("bytes=-10"), 100),
            Part::Range { start: 90, end: 99 }
        );
        assert_eq!(
            part(Some("bytes=-500"), 100),
            Part::Range { start: 0, end: 99 }
        );
        assert_eq!(
            part(Some("bytes=50-500"), 100),
            Part::Range { start: 50, end: 99 }
        );
        assert_eq!(part(Some("bytes=100-"), 100), Part::Unsatisfiable);
        assert_eq!(part(Some("bytes=-0"), 100), Part::Unsatisfiable);
        // Not served as ranges: several, reversed, or another unit
        assert_eq!(part(Some("bytes=0-1,5-6"), 100), Part::Whole);
        assert_eq!(part(Some("bytes=9-0"), 100), Part::Whole);
        assert_eq!(part(Some("items=0-9"), 100), Part::Whole);
    }
}
