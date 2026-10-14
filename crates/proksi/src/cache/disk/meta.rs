use std::{collections::BTreeMap, time::SystemTime};

use http::StatusCode;
use pingora_cache::CacheMeta;

use pingora::http::ResponseHeader;
use serde::{Deserialize, Serialize};

/// `DiskCache` storage metadata with information about the sibling cache file
#[derive(Serialize, Deserialize, Clone)]
pub struct DiskCacheItemMetadata {
    pub status: u16,
    pub created_at: SystemTime,
    pub fresh_until: SystemTime,
    pub stale_while_revalidate_sec: u32,
    pub stale_if_error_sec: u32,

    /// It's converted later on to a `ResponseHeader`
    pub headers: BTreeMap<String, String>,
}

impl DiskCacheItemMetadata {
    /// Converts a `DiskCacheItemMeta` `BTreeMap` to a `ResponseHeader`
    pub fn convert_headers(meta: &DiskCacheItemMetadata) -> pingora::Result<ResponseHeader> {
        let status_code = StatusCode::from_u16(meta.status).unwrap_or(StatusCode::OK);
        let mut res_headers = ResponseHeader::build(status_code, None)?;

        for (k, v) in &meta.headers {
            res_headers.insert_header(k.to_owned(), v).ok();
        }

        Ok(res_headers)
    }
}

impl From<&CacheMeta> for DiskCacheItemMetadata {
    /// Converts a `CacheMeta` to a `DiskCacheItemMeta`
    fn from(meta: &CacheMeta) -> Self {
        DiskCacheItemMetadata {
            status: meta.response_header().status.as_u16(),
            created_at: meta.created(),
            fresh_until: meta.fresh_until(),
            stale_while_revalidate_sec: meta.stale_while_revalidate_sec(),
            stale_if_error_sec: meta.stale_if_error_sec(),
            headers: meta
                .headers()
                .into_iter()
                // Header values aren't necessarily ASCII (e.g. UTF-8 filenames), only values that
                // can't be stored as a string are left out
                .filter_map(|(k, v)| {
                    let value = String::from_utf8(v.as_bytes().to_vec()).ok()?;
                    Some((k.to_string(), value))
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use pingora::http::ResponseHeader;
    use pingora_cache::CacheMeta;

    use super::DiskCacheItemMetadata;

    #[test]
    fn test_metadata_keeps_non_ascii_header_values() {
        let mut headers = ResponseHeader::build(200, None).unwrap();
        headers
            .insert_header("content-disposition", "attachment; filename=\"résumé.pdf\"")
            .unwrap();
        headers.insert_header("x-plain", "ascii").unwrap();

        let now = SystemTime::now();
        let meta = CacheMeta::new(now + Duration::from_secs(60), now, 0, 0, headers);

        let stored = DiskCacheItemMetadata::from(&meta);
        let restored = DiskCacheItemMetadata::convert_headers(&stored).unwrap();

        assert_eq!(
            restored
                .headers
                .get("content-disposition")
                .unwrap()
                .as_bytes(),
            "attachment; filename=\"résumé.pdf\"".as_bytes()
        );
        assert_eq!(restored.headers.get("x-plain").unwrap(), "ascii");
    }
}
