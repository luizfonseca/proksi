//! Checks shared by routes and locations

use std::str::FromStr;

use http::{HeaderName, HeaderValue};

use crate::config::{RouteHeader, RouteUpstream};

/// Upstreams need an address and a port (no upstreams is valid: requests get a `503`)
pub(super) fn validate_upstreams(upstreams: &[RouteUpstream]) -> Result<(), String> {
    for (index, upstream) in upstreams.iter().enumerate() {
        if upstream.ip.is_empty() {
            return Err(format!("upstreams[{index}].ip cannot be empty"));
        }
        if upstream.port == 0 {
            return Err(format!("upstreams[{index}].port must be greater than 0"));
        }
    }
    Ok(())
}

/// Headers to add must be valid header names and values
pub(super) fn validate_headers(headers: Option<&RouteHeader>) -> Result<(), String> {
    let headers_to_add = headers.and_then(|h| h.add.as_ref());
    for (index, header) in headers_to_add.into_iter().flatten().enumerate() {
        if HeaderName::from_str(&header.name).is_err()
            || HeaderValue::from_str(&header.value).is_err()
        {
            return Err(format!(
                "headers.add[{index}] is not a valid header: {:?}",
                header.name
            ));
        }
    }
    Ok(())
}
