//! DNS-01 challenges (ADR 0009): the TXT record `_acme-challenge.<name>` is written through the
//! DNS provider, checked on the zone's own name servers, and removed once validated

pub mod cloudflare;
mod query;

use std::{future::Future, net::SocketAddr, time::Duration};

use async_trait::async_trait;

use crate::config::{DnsChallenge, DnsProviderKind};

/// How long a record has to appear on every name server of its zone
pub const PROPAGATION_TIMEOUT: Duration = Duration::from_secs(120);

/// How often the name servers are asked while waiting
const PROPAGATION_CHECK: Duration = Duration::from_secs(2);

/// A TXT record written through a provider
#[derive(Debug, Clone, PartialEq)]
pub struct DnsRecord {
    pub name: String,
    pub value: String,
    /// The provider's ids of the zone and of the record, to remove it
    pub zone_id: String,
    pub id: String,
    /// The zone's name servers (`name`, or `ip:port`), where the record is checked
    pub name_servers: Vec<String>,
}

#[async_trait]
pub trait DnsProvider: Send + Sync {
    /// Creates the TXT record `name` with `value`
    async fn set_txt(&self, name: &str, value: &str) -> Result<DnsRecord, String>;
    async fn remove_txt(&self, record: &DnsRecord) -> Result<(), String>;
}

/// The provider of `lets_encrypt { dns { ... } }`
pub fn provider(config: &DnsChallenge) -> Box<dyn DnsProvider> {
    match config.provider {
        DnsProviderKind::Cloudflare => {
            Box::new(cloudflare::Cloudflare::new(config.api_token.clone()))
        }
    }
}

/// Where the challenge of `domain` is written: `_acme-challenge.example.com` for
/// `example.com` and `*.example.com`
pub fn challenge_name(domain: &str) -> String {
    format!(
        "_acme-challenge.{}",
        domain.strip_prefix("*.").unwrap_or(domain)
    )
}

/// Sets the record `name` to `value`, waits until its zone's name servers answer it, and runs
/// `validate`. The record is removed whatever happens.
pub async fn with_txt_record<T, F>(
    provider: &dyn DnsProvider,
    name: &str,
    value: &str,
    visible: impl AsyncFnOnce(&DnsRecord) -> Result<(), String>,
    validate: F,
) -> Result<T, String>
where
    F: Future<Output = Result<T, String>>,
{
    let record = provider.set_txt(name, value).await?;
    let result = match visible(&record).await {
        Ok(()) => validate.await,
        Err(err) => Err(err),
    };
    if let Err(err) = provider.remove_txt(&record).await {
        tracing::warn!("failed to remove the TXT record {name}: {err}");
    }
    result
}

/// Waits until every name server of the record's zone answers it, or `timeout`
pub async fn wait_until_visible(record: &DnsRecord, timeout: Duration) -> Result<(), String> {
    let servers = name_server_addresses(&record.name_servers).await?;
    let wait = async {
        loop {
            let mut visible = true;
            for server in &servers {
                let answered = query::txt_records(*server, &record.name)
                    .await
                    .is_ok_and(|records| records.contains(&record.value));
                if !answered {
                    visible = false;
                    break;
                }
            }
            if visible {
                return;
            }
            tokio::time::sleep(PROPAGATION_CHECK).await;
        }
    };
    tokio::time::timeout(timeout, wait).await.map_err(|_| {
        format!(
            "the TXT record {} wasn't visible on the name servers after {timeout:?}",
            record.name
        )
    })
}

/// The addresses of name servers given as names (port 53) or `ip:port`
async fn name_server_addresses(name_servers: &[String]) -> Result<Vec<SocketAddr>, String> {
    let mut addresses = vec![];
    for server in name_servers {
        if let Ok(address) = server.parse::<SocketAddr>() {
            addresses.push(address);
            continue;
        }
        let found = tokio::net::lookup_host((server.as_str(), 53))
            .await
            .map_err(|err| format!("couldn't resolve the name server {server}: {err}"))?
            .next()
            .ok_or_else(|| format!("the name server {server} has no address"))?;
        addresses.push(found);
    }
    if addresses.is_empty() {
        return Err("the zone has no name servers".to_string());
    }
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use std::time::Duration;

    use super::{challenge_name, wait_until_visible, with_txt_record, DnsProvider, DnsRecord};

    /// Records what it's asked to do
    #[derive(Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<String>>>,
        fail_set: bool,
    }

    #[async_trait]
    impl DnsProvider for Fake {
        async fn set_txt(&self, name: &str, value: &str) -> Result<DnsRecord, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("set {name} {value}"));
            if self.fail_set {
                return Err("refused".to_string());
            }
            Ok(DnsRecord {
                name: name.to_string(),
                value: value.to_string(),
                zone_id: "zone".to_string(),
                id: "record".to_string(),
                name_servers: vec![],
            })
        }

        async fn remove_txt(&self, record: &DnsRecord) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove {}", record.id));
            Ok(())
        }
    }

    fn record(name_servers: Vec<String>) -> DnsRecord {
        DnsRecord {
            name: "_acme-challenge.a.test".to_string(),
            value: "proof".to_string(),
            zone_id: "zone".to_string(),
            id: "record".to_string(),
            name_servers,
        }
    }

    #[tokio::test]
    async fn test_visible_once_every_name_server_answers_it() {
        let one = super::query::fake_server(vec!["other".to_string(), "proof".to_string()]).await;
        let two = super::query::fake_server(vec!["proof".to_string()]).await;
        let record = record(vec![one.to_string(), two.to_string()]);

        assert_eq!(
            wait_until_visible(&record, Duration::from_secs(5)).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn test_not_visible_on_one_name_server() {
        let one = super::query::fake_server(vec!["proof".to_string()]).await;
        let stale = super::query::fake_server(vec![]).await;
        let record = record(vec![one.to_string(), stale.to_string()]);

        let result = wait_until_visible(&record, Duration::from_millis(500)).await;
        assert_eq!(
            result,
            Err("the TXT record _acme-challenge.a.test wasn't visible on the name servers after 500ms".to_string())
        );
    }

    #[test]
    fn test_challenge_name() {
        assert_eq!(
            challenge_name("example.test"),
            "_acme-challenge.example.test"
        );
        assert_eq!(
            challenge_name("*.example.test"),
            "_acme-challenge.example.test"
        );
    }

    #[tokio::test]
    async fn test_the_record_is_removed_once_validated() {
        let fake = Fake::default();
        let result = with_txt_record(
            &fake,
            "_acme-challenge.a.test",
            "proof",
            async |_| Ok(()),
            async { Ok("certificate") },
        )
        .await;

        assert_eq!(result, Ok("certificate"));
        assert_eq!(
            *fake.calls.lock().unwrap(),
            vec!["set _acme-challenge.a.test proof", "remove record"]
        );
    }

    #[tokio::test]
    async fn test_the_record_is_removed_when_validation_fails() {
        let fake = Fake::default();
        let result: Result<(), String> = with_txt_record(
            &fake,
            "_acme-challenge.a.test",
            "proof",
            async |_| Ok(()),
            async { Err("invalid".to_string()) },
        )
        .await;

        assert_eq!(result, Err("invalid".to_string()));
        assert_eq!(fake.calls.lock().unwrap().last().unwrap(), "remove record");
    }

    #[tokio::test]
    async fn test_not_validated_until_visible() {
        let fake = Fake::default();
        let validated = Arc::new(Mutex::new(false));
        let result: Result<(), String> = with_txt_record(
            &fake,
            "_acme-challenge.a.test",
            "proof",
            async |_| Err("not visible".to_string()),
            async {
                *validated.lock().unwrap() = true;
                Ok(())
            },
        )
        .await;

        assert_eq!(result, Err("not visible".to_string()));
        assert!(!*validated.lock().unwrap());
        assert_eq!(fake.calls.lock().unwrap().last().unwrap(), "remove record");
    }

    #[tokio::test]
    async fn test_nothing_to_remove_when_the_record_is_refused() {
        let fake = Fake {
            fail_set: true,
            ..Fake::default()
        };
        let result: Result<(), String> = with_txt_record(
            &fake,
            "_acme-challenge.a.test",
            "proof",
            async |_| Ok(()),
            async { Ok(()) },
        )
        .await;

        assert_eq!(result, Err("refused".to_string()));
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
    }
}
