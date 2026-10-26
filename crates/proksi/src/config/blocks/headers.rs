use std::{borrow::Cow, collections::BTreeMap};

use serde::{Deserialize, Serialize};

use crate::config::{RouteHeader, RouteHeaderAdd, RouteHeaderRemove};

/// `headers { ... }` in a host or a location
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadersBlock {
    pub response: Option<ResponseHeadersBlock>,
    /// When the headers are changed, among the blocks of a location (0-100)
    pub priority: Option<u8>,
}

/// `response { ... }`: headers changed in responses
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResponseHeadersBlock {
    #[serde(default)]
    pub add: BTreeMap<String, String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// Response headers of the host, then of the location (so a location can override them).
/// The priority is the location's, otherwise the host's.
pub(super) fn response_headers(blocks: [Option<&HeadersBlock>; 2]) -> RouteHeader {
    let priority = blocks
        .iter()
        .rev()
        .flatten()
        .find_map(|headers| headers.priority);
    let mut add = vec![];
    let mut remove = vec![];
    for response in blocks
        .into_iter()
        .flatten()
        .filter_map(|headers| headers.response.as_ref())
    {
        add.extend(response.add.iter().map(|(name, value)| header(name, value)));
        remove.extend(response.remove.iter().map(|name| RouteHeaderRemove {
            name: Cow::Owned(name.clone()),
        }));
    }
    RouteHeader {
        add: Some(add),
        remove: Some(remove),
        priority,
    }
}

pub(super) fn header(name: &str, value: &str) -> RouteHeaderAdd {
    RouteHeaderAdd {
        name: Cow::Owned(name.to_string()),
        value: Cow::Owned(value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{HeadersBlock, ResponseHeadersBlock, response_headers};

    fn block(add: &[(&str, &str)], remove: &[&str]) -> HeadersBlock {
        HeadersBlock {
            response: Some(ResponseHeadersBlock {
                add: add
                    .iter()
                    .map(|(n, v)| ((*n).to_string(), (*v).to_string()))
                    .collect::<BTreeMap<_, _>>(),
                remove: remove.iter().map(ToString::to_string).collect(),
            }),
            priority: None,
        }
    }

    #[test]
    fn test_priority_is_the_locations_then_the_hosts() {
        let host = HeadersBlock {
            priority: Some(40),
            ..block(&[], &[])
        };
        let location = HeadersBlock {
            priority: Some(10),
            ..block(&[], &[])
        };

        assert_eq!(
            response_headers([Some(&host), Some(&location)]).priority,
            Some(10)
        );
        assert_eq!(response_headers([Some(&host), None]).priority, Some(40));
        assert_eq!(response_headers([None, None]).priority, None);
    }

    #[test]
    fn test_location_headers_come_after_host_headers() {
        let host = block(&[("x-a", "host")], &["Server"]);
        let location = block(&[("x-a", "location")], &[]);

        let headers = response_headers([Some(&host), Some(&location)]);

        let added = headers
            .add
            .unwrap()
            .into_iter()
            .map(|h| (h.name.to_string(), h.value.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(
            added,
            vec![
                ("x-a".to_string(), "host".to_string()),
                ("x-a".to_string(), "location".to_string())
            ]
        );
        assert_eq!(headers.remove.unwrap().len(), 1);
    }

    #[test]
    fn test_no_headers() {
        let headers = response_headers([None, None]);
        assert_eq!(headers.add, Some(vec![]));
        assert_eq!(headers.remove, Some(vec![]));
    }
}
