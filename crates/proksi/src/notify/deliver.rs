//! Sending an event to a webhook: a `POST`, tried again a few times when it fails

use std::{sync::LazyLock, time::Duration};

use crate::config::notify::NotifyBlock;

/// How long a webhook has to answer
const TIMEOUT: Duration = Duration::from_secs(5);

/// The waits before trying again: 4 tries in about 40s
const RETRIES: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        // A webhook answers itself: redirects could send the event (and headers) elsewhere
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("proksi/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

/// Builds the client on a thread of its own, if it isn't yet: it loads the system's TLS roots,
/// which can take seconds, and nothing should wait for that
pub async fn ready() {
    let _ = tokio::task::spawn_blocking(|| LazyLock::force(&CLIENT)).await;
}

/// Sends `body` to the block's webhook, trying again on failures (`retry`: more than once)
pub async fn send(
    block: &NotifyBlock,
    body: &serde_json::Value,
    retry: bool,
) -> Result<(), String> {
    let waits = if retry { &RETRIES[..] } else { &[] };
    let mut result = post(block, body).await;
    for wait in waits {
        if result.is_ok() {
            break;
        }
        tokio::time::sleep(*wait).await;
        result = post(block, body).await;
    }
    result
}

async fn post(block: &NotifyBlock, body: &serde_json::Value) -> Result<(), String> {
    let mut request = CLIENT.post(block.url.trim()).json(body);
    for (name, value) in &block.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.map_err(|err| {
        if err.is_timeout() {
            format!("no answer within {}s", TIMEOUT.as_secs())
        } else {
            // Without the URL, which can be a secret
            reason(&err.without_url())
        }
    })?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("answered {status}"))
    }
}

/// An error with what caused it (`error sending request: ... Connection refused`)
fn reason(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text = format!("{text}: {cause}");
        source = cause.source();
    }
    text
}

/// `url` with most of it hidden (a webhook's URL is often its secret): `https://hook…bf3a`
pub fn masked(url: &str) -> String {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let chars: Vec<char> = rest.chars().collect();
    let prefix = if scheme.is_empty() {
        String::new()
    } else {
        format!("{scheme}://")
    };
    if chars.len() <= 10 {
        return format!("{prefix}…");
    }
    let start: String = chars.iter().take(4).collect();
    let end: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{prefix}{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::masked;

    #[test]
    fn test_urls_are_masked() {
        assert_eq!(
            masked("https://hooks.slack.com/services/T000/B000/XXXXbf3a"),
            "https://hook…bf3a"
        );
        assert_eq!(masked("http://short.io/"), "http://…");
        assert_eq!(
            masked("  https://example.com/abcdefgh  "),
            "https://exam…efgh"
        );
    }
}
