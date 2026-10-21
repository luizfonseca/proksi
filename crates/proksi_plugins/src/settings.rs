//! Each block's settings, as written in a location or a rule's `then`: what a block accepts (an
//! unknown or mistyped setting is an error), and the schema the admin UI builds its forms from

use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize};

use crate::RoutePlugin;

/// When a block runs among the blocks of its location, from 0 (first) to 100
type Priority = Option<u8>;

/// A block's settings, or why they're invalid (`rate_limit: unknown field "burst"`)
///
/// # Errors
///
/// When the settings don't match the block's, with its name in the message.
pub fn read<T: DeserializeOwned>(name: &str, plugin: &RoutePlugin) -> Result<T, String> {
    let settings = plugin
        .config
        .iter()
        .flatten()
        .map(|(key, value)| (Cow::into_owned(key.clone()), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    serde_json::from_value(serde_json::Value::Object(settings)).map_err(|err| {
        let err = err.to_string();
        match err.strip_prefix("missing field ") {
            Some(field) => format!("{name}: {field} is required"),
            None => format!("{name}: {err}"),
        }
    })
}

/// `rate_limit`: answers `429` to clients over the limit
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RateLimit {
    /// Requests allowed for each client in each period (more than 0)
    pub requests: u64,
    /// The period, such as "10s", "1m" or "1h"
    pub per: String,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `bandwidth_limit`: paces the responses sent to each client
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BandwidthLimit {
    /// Bytes per second for each client, such as "512KiB" or "1MiB"
    pub rate: String,
    /// Bytes sent at full speed first, such as "10MiB" (default: one second of `rate`)
    pub burst: Option<String>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `body_limit`: answers `413` to requests with a larger body
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BodyLimit {
    /// The largest body accepted, such as "512KiB", "1MiB" or "10MB"
    pub max: String,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `redirect`: sends requests elsewhere
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Redirect {
    /// Where requests are sent; `{uri}`, `{path}`, `{query}` and `{host}` are replaced with the
    /// request's
    pub to: String,
    /// 301, 302, 303, 307 or 308 (default: 302)
    pub status: Option<u16>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `respond`: answers requests itself
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Respond {
    /// The status, from 200 to 599 (default: 200)
    pub status: Option<u16>,
    /// The body (default: none)
    pub body: Option<String>,
    /// Its `Content-Type`, such as "text/plain"
    pub content_type: Option<String>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `ip_filter`: answers `403` to clients it doesn't accept
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IpFilter {
    /// Addresses and networks accepted ("203.0.113.9", "10.0.0.0/8"); when set, every other
    /// client is refused
    pub allow: Option<Vec<String>>,
    /// Addresses and networks refused, even when `allow` includes them
    pub deny: Option<Vec<String>>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `cors`: answers browsers' cross-origin requests
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cors {
    /// The origins accepted: exact (`https://app.example.com`), subdomains
    /// (`https://*.example.com`) or any (`*`)
    #[serde(alias = "allowed_origins")]
    pub origins: Vec<String>,
    /// The methods allowed (default: GET, HEAD, POST, PUT, PATCH, DELETE)
    pub methods: Option<Vec<String>>,
    /// The request headers allowed (default: those the browser asks for)
    pub headers: Option<Vec<String>>,
    /// Response headers the page can read, besides the basic ones
    pub expose_headers: Option<Vec<String>>,
    /// Whether the browser sends cookies and `Authorization` (default: false)
    pub credentials: Option<bool>,
    /// How long browsers keep a preflight's answer, in seconds
    pub max_age_secs: Option<u64>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// A compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Zstd,
    Br,
    Gzip,
}

/// `compress`: compresses responses
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Compress {
    /// The algorithms used (default: all three)
    pub algorithms: Option<Vec<Algorithm>>,
    /// The level of every algorithm (default: 3 for zstd, 4 for brotli, 6 for gzip)
    pub level: Option<u32>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `forward_auth`: asks another service whether requests may go through
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForwardAuth {
    /// The auth service's endpoint (http or https)
    pub url: String,
    /// Headers of its answer added to the request sent to the upstream
    pub response_headers: Option<Vec<String>>,
    /// Only these headers of the request are sent to it (default: all)
    pub request_headers: Option<Vec<String>>,
    /// How long it has to answer, in seconds (default: 5)
    pub timeout_secs: Option<u64>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `jwt`: lets through requests with a valid JSON Web Token, and answers `401` to the others
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Jwt {
    /// The shared secret of HMAC tokens (HS256, HS384, HS512)
    pub secret: Option<String>,
    /// The public key of signed tokens, in PEM (RSA, ECDSA or Ed25519)
    pub public_key: Option<String>,
    /// Where the issuer publishes its public keys (JWKS), fetched in the background
    pub jwks_url: Option<String>,
    /// The issuer tokens must name (`iss`)
    pub issuer: Option<String>,
    /// Tokens must name one of these audiences (`aud`)
    pub audience: Option<Vec<String>>,
    /// Read the token from this cookie, instead of `Authorization: Bearer`
    pub cookie: Option<String>,
    /// Claims sent to the upstream as headers, by claim (`{ sub = "X-User-Id" }`). Headers of
    /// the same names sent by the client are removed.
    pub claims_to_headers: Option<std::collections::BTreeMap<String, String>>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `static_files`: answers with the files of a directory, by their path below the location
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticFiles {
    /// The directory files are served from (an absolute path)
    pub directory: String,
    /// Files served for a directory's path (default: `["index.html"]`)
    pub index: Option<Vec<String>>,
    /// The file served for paths with none, such as `/index.html` for single-page apps
    /// (default: `404`)
    pub fallback: Option<String>,
    /// The `Cache-Control` of the files, such as `public, max-age=3600`
    pub cache_control: Option<String>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `basic_auth`: asks for a user and password
// Read by its plugin, from the settings as written
#[allow(dead_code)]
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BasicAuth {
    pub user: String,
    pub pass: String,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// An `oauth2` provider
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    Workos,
}

/// What an `oauth2` validation checks
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Email,
    Username,
    TeamId,
    OrgId,
}

impl Check {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Check::Email => "email",
            Check::Username => "username",
            Check::TeamId => "team_id",
            Check::OrgId => "org_id",
        }
    }
}

/// Users are let in only if their `type` is one of `value`
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Validation {
    /// `key` in earlier documentation
    #[serde(rename = "type", alias = "key")]
    pub check: Check,
    pub value: Vec<String>,
}

/// `oauth2`: signs users in with an `OAuth2` provider
// Read by its plugin, from the settings as written
#[allow(dead_code)]
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Oauth2 {
    pub provider: Provider,
    pub client_id: String,
    pub client_secret: String,
    /// Signs the session cookie (at least 64 characters)
    pub jwt_secret: String,
    /// Who's let in (default: anyone the provider signs in)
    pub validations: Option<Vec<Validation>>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// `request_id`: gives each request an ID, as `X-Request-Id`
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestId {
    /// Added to the IDs proksi makes, after a `-`, such as the server's name or region (1 to
    /// 32 letters, digits, `-`, `_` or `.`)
    pub suffix: Option<String>,
    /// The header the ID is sent and kept in (default: `X-Request-Id`)
    pub header_name: Option<String>,
    // Read by `plugin_priority`
    #[allow(dead_code)]
    #[schemars(
        description = "When it runs among the blocks of its location, from 0 (first) to 100",
        range(max = 100)
    )]
    pub priority: Priority,
}

/// Each block's settings, as JSON Schema, by name
#[must_use]
pub fn schemas() -> serde_json::Map<String, serde_json::Value> {
    let schema = |schema: schemars::Schema| schema.to_value();
    [
        ("rate_limit", schema(schemars::schema_for!(RateLimit))),
        ("bandwidth_limit", schema(schemars::schema_for!(BandwidthLimit))),
        ("body_limit", schema(schemars::schema_for!(BodyLimit))),
        ("redirect", schema(schemars::schema_for!(Redirect))),
        ("respond", schema(schemars::schema_for!(Respond))),
        ("ip_filter", schema(schemars::schema_for!(IpFilter))),
        ("cors", schema(schemars::schema_for!(Cors))),
        ("compress", schema(schemars::schema_for!(Compress))),
        ("forward_auth", schema(schemars::schema_for!(ForwardAuth))),
        ("jwt", schema(schemars::schema_for!(Jwt))),
        ("static_files", schema(schemars::schema_for!(StaticFiles))),
        ("basic_auth", schema(schemars::schema_for!(BasicAuth))),
        ("oauth2", schema(schemars::schema_for!(Oauth2))),
        ("request_id", schema(schemars::schema_for!(RequestId))),
    ]
    .into_iter()
    .map(|(name, schema)| (name.to_string(), schema))
    .collect()
}

#[cfg(test)]
mod tests {
    use crate::RoutePlugin;

    use super::{read, schemas, Check, Oauth2, RateLimit};

    fn plugin(name: &str, config: serde_json::Value) -> RoutePlugin {
        serde_json::from_value(serde_json::json!({ "name": name, "config": config })).unwrap()
    }

    #[test]
    fn test_unknown_and_mistyped_settings() {
        let read_limit =
            |config| read::<RateLimit>("rate_limit", &plugin("rate_limit", config)).map(|_| ());

        assert_eq!(read_limit(serde_json::json!({ "requests": 10, "per": "1m" })), Ok(()));
        let unknown = read_limit(serde_json::json!({ "requests": 10, "per": "1m", "burst": 5 }));
        assert!(
            unknown.as_ref().is_err_and(|err| err.starts_with("rate_limit: unknown field `burst`")),
            "{unknown:?}"
        );
        let mistyped = read_limit(serde_json::json!({ "requests": "ten", "per": "1m" }));
        assert!(mistyped.is_err_and(|err| err.starts_with("rate_limit: invalid type")));
        assert_eq!(
            read_limit(serde_json::json!({ "per": "1m" })),
            Err("rate_limit: `requests` is required".to_string())
        );
    }

    #[test]
    fn test_oauth2_validations_as_documented() {
        // `key`, as the documentation showed, is `type`
        let oauth2 = read::<Oauth2>(
            "oauth2",
            &plugin(
                "oauth2",
                serde_json::json!({
                    "provider": "github",
                    "client_id": "id",
                    "client_secret": "secret",
                    "jwt_secret": "jwt",
                    "validations": [{ "key": "email", "value": ["a@example.com"] }],
                }),
            ),
        )
        .unwrap();
        assert_eq!(oauth2.validations.unwrap()[0].check, Check::Email);
    }

    #[test]
    fn test_schemas() {
        let schemas = schemas();
        let limit = &schemas["rate_limit"];
        assert_eq!(limit["required"], serde_json::json!(["requests", "per"]));
        assert_eq!(limit["properties"]["requests"]["type"], "integer");
        assert_eq!(limit["additionalProperties"], false);
        assert!(
            limit["properties"]["per"]["description"]
                .as_str()
                .is_some_and(|text| text.contains("1m")),
            "{limit}"
        );
        assert_eq!(limit["properties"]["priority"]["maximum"], 100);
        assert_eq!(schemas["jwt"]["additionalProperties"], false);
        assert_eq!(schemas.len(), 14);
    }
}
