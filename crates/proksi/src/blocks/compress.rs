//! `compress { }`: compresses responses with gzip, brotli or zstd, as the client accepts

use anyhow::Result;
use async_trait::async_trait;
use http::header;
use pingora::{
    http::RequestHeader, modules::http::compression::ResponseCompression,
    protocols::http::compression::Algorithm, proxy::Session,
};

use crate::{config::RoutePlugin, proxy_server::https_proxy::RouterContext};

use super::Middleware;

const INVALID_ALGORITHMS: &str =
    "compress: `algorithms` must be a list of \"zstd\", \"br\" and \"gzip\"";

/// The algorithms, their default level and the highest level they accept
const ALGORITHMS: [(&str, Algorithm, u32, u32); 3] = [
    ("zstd", Algorithm::Zstd, 3, 22),
    ("br", Algorithm::Brotli, 4, 11),
    ("gzip", Algorithm::Gzip, 6, 9),
];

/// Turns on Pingora's response compression for the request, at the level of each algorithm the
/// location allows (`0`: off). Of the algorithms the client accepts, the best one is used
/// (zstd, then brotli, then gzip), whatever order the client lists them in. Responses that are
/// already compressed, too small or of a type that doesn't compress well (images, archives)
/// are sent as they are.
pub(super) struct Compress {
    levels: Vec<(Algorithm, u32)>,
}

impl Compress {
    pub(super) fn new(plugin: &RoutePlugin) -> Result<Self, String> {
        let setting = |key: &str| plugin.config.as_ref().and_then(|config| config.get(key));

        let allowed = match setting("algorithms") {
            None => ALGORITHMS.iter().map(|(name, ..)| *name).collect(),
            Some(value) => value
                .as_array()
                .filter(|names| !names.is_empty())
                .and_then(|names| names.iter().map(serde_json::Value::as_str).collect())
                .filter(|names: &Vec<&str>| {
                    names
                        .iter()
                        .all(|name| ALGORITHMS.iter().any(|(known, ..)| known == name))
                })
                .ok_or(INVALID_ALGORITHMS)?,
        };
        let level = setting("level")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|level| u32::try_from(level).ok())
                    .filter(|level| *level > 0)
                    .ok_or("compress: `level` must be a number greater than 0")
            })
            .transpose()?;

        let levels = ALGORITHMS
            .iter()
            .map(|(name, algorithm, default, max)| {
                if !allowed.contains(name) {
                    return Ok((*algorithm, 0));
                }
                match level {
                    Some(level) if level > *max => Err(format!(
                        "compress: `level` {level} is above the highest {name} level ({max})"
                    )),
                    Some(level) => Ok((*algorithm, level)),
                    None => Ok((*algorithm, *default)),
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self { levels })
    }
}

#[async_trait]
impl Middleware for Compress {
    async fn request(&self, session: &mut Session, _ctx: &mut RouterContext) -> Result<bool> {
        let Some(compression) = session
            .downstream_modules_ctx
            .get_mut::<ResponseCompression>()
        else {
            return Ok(false);
        };
        let accepted = session
            .downstream_session
            .req_header()
            .headers
            .get(header::ACCEPT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| self.best(value));
        let Some(name) = accepted else {
            return Ok(false);
        };
        for (algorithm, level) in &self.levels {
            compression.adjust_algorithm_level(*algorithm, *level);
        }
        // Pingora uses the first algorithm of `Accept-Encoding` (and it read the request while
        // compression was off): it's given the one chosen
        let mut request = RequestHeader::build("GET", b"/", Some(1))?;
        request.insert_header(header::ACCEPT_ENCODING, name)?;
        compression.request_filter(&request);
        Ok(false)
    }
}

impl Compress {
    /// The best algorithm of the location that `accept_encoding` accepts
    fn best(&self, accept_encoding: &str) -> Option<&'static str> {
        let accepts = |name: &str| {
            accept_encoding.split(',').any(|coding| {
                let mut parts = coding.split(';');
                let accepted = parts
                    .next()
                    .is_some_and(|coding| coding.trim().eq_ignore_ascii_case(name));
                // `q=0`: not acceptable
                let refused = parts.any(|parameter| {
                    parameter
                        .trim()
                        .strip_prefix("q=")
                        .and_then(|q| q.trim().parse::<f32>().ok())
                        .is_some_and(|q| q <= 0.0)
                });
                accepted && !refused
            })
        };
        ALGORITHMS
            .iter()
            .zip(&self.levels)
            .find(|((name, ..), (_, level))| *level > 0 && accepts(name))
            .map(|((name, ..), _)| *name)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use pingora::protocols::http::compression::Algorithm;

    use crate::config::RoutePlugin;

    use super::Compress;

    fn compress(config: serde_json::Value) -> Result<Vec<(Algorithm, u32)>, String> {
        Compress::new(&RoutePlugin {
            name: Cow::Borrowed("compress"),
            config: serde_json::from_value(config).unwrap(),
        })
        .map(|compress| compress.levels)
    }

    #[test]
    fn test_every_algorithm_by_default() {
        assert_eq!(
            compress(serde_json::json!({})).unwrap(),
            vec![
                (Algorithm::Zstd, 3),
                (Algorithm::Brotli, 4),
                (Algorithm::Gzip, 6)
            ]
        );
    }

    #[test]
    fn test_algorithms_and_level() {
        assert_eq!(
            compress(serde_json::json!({ "algorithms": ["gzip"], "level": 9 })).unwrap(),
            vec![
                (Algorithm::Zstd, 0),
                (Algorithm::Brotli, 0),
                (Algorithm::Gzip, 9)
            ]
        );
    }

    #[test]
    fn test_best_algorithm_the_client_accepts() {
        let all = Compress::new(&RoutePlugin {
            name: Cow::Borrowed("compress"),
            config: None,
        })
        .unwrap();

        assert_eq!(all.best("gzip, deflate, br, zstd"), Some("zstd"));
        assert_eq!(all.best("gzip;q=1.0, br;q=0.5"), Some("br"));
        assert_eq!(all.best("zstd;q=0, gzip"), Some("gzip"));
        assert_eq!(all.best("GZIP"), Some("gzip"));
        assert_eq!(all.best("identity"), None);
        assert_eq!(all.best(""), None);

        let gzip = Compress::new(&RoutePlugin {
            name: Cow::Borrowed("compress"),
            config: serde_json::from_value(serde_json::json!({ "algorithms": ["gzip"] })).unwrap(),
        })
        .unwrap();
        assert_eq!(gzip.best("zstd, br"), None);
        assert_eq!(gzip.best("zstd, gzip"), Some("gzip"));
    }

    #[test]
    fn test_invalid_settings() {
        for config in [
            serde_json::json!({ "algorithms": ["deflate"] }),
            serde_json::json!({ "algorithms": [] }),
            serde_json::json!({ "algorithms": "gzip" }),
        ] {
            assert_eq!(
                compress(config).unwrap_err(),
                super::INVALID_ALGORITHMS.to_string()
            );
        }
        assert!(compress(serde_json::json!({ "level": 0 })).is_err());
        // Above brotli's highest level
        assert_eq!(
            compress(serde_json::json!({ "level": 12 })).unwrap_err(),
            "compress: `level` 12 is above the highest br level (11)"
        );
    }
}
