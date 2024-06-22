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
    pub fn convert_headers(meta: &DiskCacheItemMetadata) -> ResponseHeader {
        let status_code = StatusCode::from_u16(meta.status).unwrap_or(StatusCode::OK);
        let mut res_headers = ResponseHeader::build(status_code, None).unwrap();

        for (k, v) in &meta.headers {
            res_headers.insert_header(k.to_owned(), v).ok();
        }

        res_headers
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
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
                .collect(),
        }
    }
}
