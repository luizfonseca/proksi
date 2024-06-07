use std::{borrow::Cow, collections::HashMap};

/// Converts a serde_json::Value to a String (without the quotes)
pub(super) fn from_json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        // When Value is a String, it returns a quoted string i.e. "value"
        // which we then need to converto to &str to get the data without them
        serde_json::Value::String(s) => s.as_str().to_string(),
        _ => "".to_string(),
    }
}

/// Parses an HTTP str (from http::Uri) to a Hashmap of query parameters
pub(super) fn from_string_to_query_params(value: &str) -> HashMap<Cow<str>, Cow<str>> {
    value
        .split('&')
        .map(|kv| {
            let mut kv = kv.split('=');
            (
                Cow::Borrowed(kv.next().unwrap()),
                Cow::Borrowed(kv.next().unwrap()),
            )
        })
        .collect()
}
