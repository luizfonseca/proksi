//! `then { ... }`: the blocks of a matched rule (the location's own, reused as they are), and
//! changes to the request (its method and headers) that the next rules and the upstream see

use std::{borrow::Cow, collections::BTreeMap, collections::HashMap, str::FromStr};

use http::{HeaderName, HeaderValue, Method};
use pingora::http::RequestHeader;
use serde::Deserialize;

use crate::{
    blocks::Blocks,
    config::{RouteHeader, RouteHeaderAdd, RouteHeaderRemove, RoutePlugin, host::Location},
};

type PluginConfig = HashMap<Cow<'static, str>, serde_json::Value>;

/// `then { ... }` as written
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ThenBlock {
    #[schemars(with = "Option<proksi_plugins::settings::Redirect>")]
    redirect: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Respond>")]
    respond: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::IpFilter>")]
    ip_filter: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::RateLimit>")]
    rate_limit: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BandwidthLimit>")]
    bandwidth_limit: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::WaitingRoom>")]
    waiting_room: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BodyRewrite>")]
    body_rewrite: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::OpenApi>")]
    openapi: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BodyLimit>")]
    body_limit: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::BasicAuth>")]
    basic_auth: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Jwt>")]
    jwt: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Oauth2>")]
    oauth2: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::ForwardAuth>")]
    forward_auth: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Cors>")]
    cors: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::Compress>")]
    compress: Option<PluginConfig>,
    #[schemars(with = "Option<proksi_plugins::settings::RequestId>")]
    request_id: Option<PluginConfig>,
    headers: Option<HeaderChanges>,
    method: Option<MethodChange>,
    /// Sends the request to this traffic split of its location, whatever its share
    traffic_split: Option<String>,
    /// Copies the request to its location's mirror (true), or doesn't (false), whatever its share
    mirror: Option<bool>,
    /// Whether the request can be sent to another upstream when one fails (false: never, such as for payments)
    retry: Option<bool>,
}

/// `headers`: changes the request's headers, or the response's
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct HeaderChanges {
    /// The request's headers, as the next rules and the upstream see them
    request: Option<Changes>,
    /// The response's headers
    response: Option<Changes>,
    /// When the response's are changed among the blocks, from 0 to 100
    #[schemars(range(max = 100))]
    priority: Option<u8>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Changes {
    /// Headers added, replacing any with the same name
    #[serde(default)]
    add: BTreeMap<String, String>,
    /// Headers removed
    #[serde(default)]
    remove: Vec<String>,
}

/// `method`: changes the request's method
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct MethodChange {
    /// The method sent to the upstream, such as "DELETE"
    set: String,
}

/// What `then` takes: each block (the settings of a location's), and changes to the request, as
/// JSON Schema
pub fn schema() -> serde_json::Map<String, serde_json::Value> {
    let mut actions = crate::blocks::settings::schemas();
    // Files are found below a location's path, which rules don't have
    actions.remove("static_files");
    actions.insert(
        "headers".to_string(),
        schemars::schema_for!(HeaderChanges).to_value(),
    );
    actions.insert(
        "method".to_string(),
        schemars::schema_for!(MethodChange).to_value(),
    );
    actions.insert(
        "traffic_split".to_string(),
        serde_json::json!({
            "type": "string",
            "description": "Sends the request to this traffic split of its location, whatever its share",
        }),
    );
    actions.insert(
        "mirror".to_string(),
        serde_json::json!({
            "type": "boolean",
            "description": "Copies the request to its location's mirror (true), or doesn't (false), whatever its share",
        }),
    );
    actions.insert(
        "retry".to_string(),
        serde_json::json!({
            "type": "boolean",
            "description": "Whether the request can be sent to another upstream when one fails (false: never, such as for payments)",
        }),
    );
    actions
}

/// A location holding the blocks of `then`, to build them as a location's
fn blocks_location(
    plugins: Vec<RoutePlugin>,
    response: &Changes,
    priority: Option<u8>,
) -> Location {
    let add = response
        .add
        .iter()
        .map(|(name, value)| RouteHeaderAdd {
            name: Cow::Owned(name.clone()),
            value: Cow::Owned(value.clone()),
        })
        .collect();
    let remove = response
        .remove
        .iter()
        .map(|name| RouteHeaderRemove {
            name: Cow::Owned(name.clone()),
        })
        .collect();
    Location {
        path: String::new(),
        exact: false,
        upstreams: vec![],
        headers: Some(RouteHeader {
            add: Some(add),
            remove: Some(remove),
            priority,
        }),
        plugins: Some(plugins),
        cache: None,
        timeouts: None,
        rewrite: None,
        health_check: None,
        rules: vec![],
        splits: vec![],
        mirror: None,
        load_balancing: crate::config::host::LoadBalancing::RoundRobin,
        retry: None,
    }
}

type HeaderChange = (Vec<(HeaderName, HeaderValue)>, Vec<HeaderName>);

/// The request headers `then` adds and removes
fn request_changes(request: &Changes) -> Result<HeaderChange, String> {
    let add = request
        .add
        .iter()
        .map(
            |(name, value)| match (HeaderName::from_str(name), HeaderValue::from_str(value)) {
                (Ok(name), Ok(value)) => Ok((name, value)),
                _ => Err(format!("headers: {name:?} is not a valid header")),
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    let remove = request
        .remove
        .iter()
        .map(|name| {
            HeaderName::from_str(name)
                .map_err(|_| format!("headers: {name:?} is not a header name"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((add, remove))
}

/// What a matched rule does
pub(crate) struct Then {
    pub(super) blocks: Blocks,
    method: Option<Method>,
    /// The traffic split the request goes to
    pub(super) split: Option<std::sync::Arc<str>>,
    /// Whether the request is copied to its location's mirror
    pub(super) mirror: Option<bool>,
    /// Whether the request can be tried again on another upstream
    pub(super) retry: Option<bool>,
    add: Vec<(HeaderName, HeaderValue)>,
    remove: Vec<HeaderName>,
}

impl Then {
    /// `id` says where the rule is written (see `rule_id`)
    pub(super) fn build(value: &serde_json::Value, id: &str) -> Result<Self, String> {
        let then = ThenBlock::deserialize(value).map_err(|err| err.to_string())?;

        let plugins = [
            ("redirect", then.redirect),
            ("respond", then.respond),
            ("ip_filter", then.ip_filter),
            ("rate_limit", then.rate_limit),
            ("bandwidth_limit", then.bandwidth_limit),
            ("waiting_room", then.waiting_room),
            ("body_rewrite", then.body_rewrite),
            ("openapi", then.openapi),
            ("body_limit", then.body_limit),
            ("basic_auth", then.basic_auth),
            ("jwt", then.jwt),
            ("oauth2", then.oauth2),
            ("forward_auth", then.forward_auth),
            ("cors", then.cors),
            ("compress", then.compress),
            ("request_id", then.request_id),
        ]
        .into_iter()
        .filter_map(|(name, config)| {
            config.map(|config| RoutePlugin {
                name: Cow::Borrowed(name),
                config: Some(config),
            })
        })
        .collect::<Vec<_>>();

        let (request, response, priority) = match then.headers {
            Some(headers) => (
                headers.request.unwrap_or_default(),
                headers.response.unwrap_or_default(),
                headers.priority,
            ),
            None => (Changes::default(), Changes::default(), None),
        };
        let blocks = Blocks::build(&blocks_location(plugins, &response, priority), id)?;
        let (add, remove) = request_changes(&request)?;
        let method = then
            .method
            .map(|method| {
                Method::from_bytes(method.set.to_ascii_uppercase().as_bytes())
                    .map_err(|_| format!("method: {:?} isn't a method", method.set))
            })
            .transpose()?;

        let split = match then.traffic_split {
            Some(name) if name.trim().is_empty() => {
                return Err(
                    "traffic_split: the name of a traffic split of the location".to_string()
                );
            }
            name => name.map(std::sync::Arc::from),
        };

        let then = Self {
            blocks,
            method,
            split,
            mirror: then.mirror,
            retry: then.retry,
            add,
            remove,
        };
        if then.blocks.iter().next().is_none()
            && then.method.is_none()
            && then.split.is_none()
            && then.mirror.is_none()
            && then.retry.is_none()
            && then.add.is_empty()
            && then.remove.is_empty()
        {
            return Err(
                "does nothing: add what the rule does, such as `redirect { ... }`".to_string(),
            );
        }
        Ok(then)
    }

    /// Changes the request (its method and headers): whether anything changed
    pub(super) fn change_request(&self, request: &mut RequestHeader) -> bool {
        if self.method.is_none() && self.add.is_empty() && self.remove.is_empty() {
            return false;
        }
        if let Some(method) = &self.method {
            request.set_method(method.clone());
        }
        for name in &self.remove {
            request.remove_header(name);
        }
        for (name, value) in &self.add {
            // `insert_header` only fails for invalid names and values, checked when built
            let _ = request.insert_header(name.clone(), value.clone());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Then;

    fn err(then: serde_json::Value) -> String {
        Then::build(&then, "").err().unwrap_or_default()
    }

    #[test]
    fn test_then_turns_retries_off() {
        let then = Then::build(&json!({ "retry": false }), "").unwrap();
        assert_eq!(then.retry, Some(false));
        assert!(Then::build(&json!({ "retry": 2 }), "").is_err());
    }

    #[test]
    fn test_then_chooses_to_mirror() {
        let then = Then::build(&json!({ "mirror": true }), "").unwrap();
        assert_eq!(then.mirror, Some(true));
        let then = Then::build(&json!({ "mirror": false }), "").unwrap();
        assert_eq!(then.mirror, Some(false));
        assert!(Then::build(&json!({ "mirror": "yes" }), "").is_err());
    }

    #[test]
    fn test_then_chooses_a_traffic_split() {
        let then = Then::build(&json!({ "traffic_split": "v2" }), "").unwrap();
        assert_eq!(then.split.as_deref(), Some("v2"));
        assert_eq!(
            err(json!({ "traffic_split": "" })),
            "traffic_split: the name of a traffic split of the location"
        );
    }

    #[test]
    fn test_then_takes_location_blocks() {
        let then = Then::build(
            &json!({
                "rate_limit": { "requests": 60, "per": "1m" },
                "headers": { "response": { "add": { "cache-control": "no-store" } } },
            }),
            "",
        )
        .unwrap();
        let kinds = then
            .blocks
            .iter()
            .map(|block| block.kind.name())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["rate_limit", "headers"]);
    }

    #[test]
    fn test_request_changes() {
        let then = Then::build(&json!({
            "method": { "set": "delete" },
            "headers": { "request": { "add": { "x-a": "1" }, "remove": ["x-http-method-override"] } },
        }), "")
        .unwrap();
        let mut request = pingora::http::RequestHeader::build("POST", b"/", None).unwrap();
        request
            .insert_header("x-http-method-override", "DELETE")
            .unwrap();

        assert!(then.change_request(&mut request));
        assert_eq!(request.method, http::Method::DELETE);
        assert!(request.headers.get("x-http-method-override").is_none());
        assert_eq!(request.headers.get("x-a").unwrap(), "1");
    }

    #[test]
    fn test_invalid_then() {
        assert!(err(json!({ "upstream": {} })).starts_with("unknown field `upstream`"));
        assert_eq!(
            err(json!({})),
            "does nothing: add what the rule does, such as `redirect { ... }`"
        );
        assert_eq!(err(json!({ "redirect": {} })), "redirect: `to` is required");
        assert_eq!(
            err(json!({ "method": { "set": "GE T" } })),
            "method: \"GE T\" isn't a method"
        );
    }
}
