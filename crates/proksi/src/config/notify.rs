//! `notify "<name>" { url = "...", on = [...] }`: webhooks told about what happens to Proksi
//! (upstreams going down, certificates failing, configuration reloads...)

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What Proksi tells a webhook about
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A health check found an upstream failing
    UpstreamDown,
    /// A failing upstream passes its health checks again
    UpstreamUp,
    /// Let's Encrypt gave a certificate for a new domain
    CertificateIssued,
    /// A certificate was renewed
    CertificateRenewed,
    /// Ordering or renewing a certificate failed
    CertificateFailed,
    /// A certificate has fewer days left than `expiring_days`
    CertificateExpiring,
    /// The configuration file changed, and was applied
    ConfigReloaded,
    /// The configuration file changed, but is invalid: it was kept as it was
    ConfigReloadFailed,
    /// A host in the store was saved, removed or rolled back
    HostChanged,
    /// The shared store stopped answering
    StoreDown,
    /// The shared store answers again
    StoreUp,
    /// An instance started serving
    InstanceStarted,
    /// An instance is shutting down
    InstanceStopping,
    /// Sent from the admin UI or API, to check a webhook
    Test,
}

impl Kind {
    /// Every event a `notify` block can ask for (`Test` is sent on request only)
    pub const ALL: [Kind; 13] = [
        Kind::UpstreamDown,
        Kind::UpstreamUp,
        Kind::CertificateIssued,
        Kind::CertificateRenewed,
        Kind::CertificateFailed,
        Kind::CertificateExpiring,
        Kind::ConfigReloaded,
        Kind::ConfigReloadFailed,
        Kind::HostChanged,
        Kind::StoreDown,
        Kind::StoreUp,
        Kind::InstanceStarted,
        Kind::InstanceStopping,
    ];

    /// As written in the configuration
    pub fn name(self) -> &'static str {
        match self {
            Kind::UpstreamDown => "upstream_down",
            Kind::UpstreamUp => "upstream_up",
            Kind::CertificateIssued => "certificate_issued",
            Kind::CertificateRenewed => "certificate_renewed",
            Kind::CertificateFailed => "certificate_failed",
            Kind::CertificateExpiring => "certificate_expiring",
            Kind::ConfigReloaded => "config_reloaded",
            Kind::ConfigReloadFailed => "config_reload_failed",
            Kind::HostChanged => "host_changed",
            Kind::StoreDown => "store_down",
            Kind::StoreUp => "store_up",
            Kind::InstanceStarted => "instance_started",
            Kind::InstanceStopping => "instance_stopping",
            Kind::Test => "test",
        }
    }
}

/// How the request to the webhook is written
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// The event as a JSON object (`event`, `summary`, `host`, ...)
    #[default]
    Json,
    /// A message for a Slack incoming webhook (`{"text": ...}`)
    Slack,
    /// A message for a Discord webhook (`{"content": ...}`)
    Discord,
}

fn default_expiring_days() -> u32 {
    14
}

/// `notify "<name>" { ... }`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotifyBlock {
    /// Where events are sent, with a `POST`
    pub url: String,
    #[serde(default)]
    pub format: Format,
    /// The events sent: all of them when left out
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<Kind>,
    /// Only the events of these hosts (`api.example.com`, `*.example.com`, `*`): events that
    /// aren't about a host (the store, an instance, the configuration) are always sent
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// Headers added to each request, such as `{ authorization = env("TOKEN") }`
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// `certificate_expiring` is sent once a day when a certificate has fewer days left
    #[serde(default = "default_expiring_days")]
    pub expiring_days: u32,
}

impl NotifyBlock {
    /// Whether this block asks for `kind`
    pub fn wants(&self, kind: Kind) -> bool {
        kind == Kind::Test || self.on.is_empty() || self.on.contains(&kind)
    }

    /// Whether an event about `host` (none: not about a host) is for this block
    pub fn covers(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return true;
        };
        self.hosts.is_empty() || self.hosts.iter().any(|pattern| host_matches(pattern, host))
    }

    /// Why this block can't be used, if it can't
    pub fn check(&self, name: &str) -> Result<(), String> {
        let url = self.url.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!(
                "notify \"{name}\": url must start with https:// or http://"
            ));
        }
        for pattern in &self.hosts {
            if pattern.is_empty()
                || (pattern.contains('*') && pattern != "*" && !pattern.starts_with("*."))
            {
                return Err(format!(
                    "notify \"{name}\": hosts patterns are names, `*.<name>` or `*`, not {pattern:?}"
                ));
            }
        }
        for header in self.headers.keys() {
            if http::HeaderName::try_from(header.as_str()).is_err() {
                return Err(format!(
                    "notify \"{name}\": {header:?} is not a header name"
                ));
            }
        }
        Ok(())
    }
}

/// `*` matches every host, `*.example.com` one more label (as wildcard hosts do), anything
/// else the host with that name
pub fn host_matches(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(parent) = pattern.strip_prefix("*.") {
        return host
            .split_once('.')
            .is_some_and(|(label, rest)| !label.is_empty() && rest.eq_ignore_ascii_case(parent));
    }
    pattern.eq_ignore_ascii_case(host)
}

#[cfg(test)]
mod tests {
    use super::{Kind, NotifyBlock, host_matches};

    fn block(on: &[Kind], hosts: &[&str]) -> NotifyBlock {
        NotifyBlock {
            url: "https://hooks.example.com/a".into(),
            format: super::Format::Json,
            on: on.to_vec(),
            hosts: hosts.iter().map(|host| (*host).to_string()).collect(),
            headers: Default::default(),
            expiring_days: 14,
        }
    }

    #[test]
    fn test_hosts_patterns() {
        assert!(host_matches("*", "a.example.com"));
        assert!(host_matches("api.example.com", "API.example.com"));
        assert!(host_matches("*.example.com", "a.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        assert!(!host_matches("*.example.com", "a.b.example.com"));
        assert!(!host_matches("api.example.com", "app.example.com"));
    }

    #[test]
    fn test_which_events_a_block_gets() {
        let everything = block(&[], &[]);
        assert!(Kind::ALL.iter().all(|kind| everything.wants(*kind)));
        assert!(everything.covers(Some("any.test")) && everything.covers(None));

        let payments = block(
            &[Kind::UpstreamDown],
            &["pay.example.com", "*.payments.internal"],
        );
        assert!(payments.wants(Kind::UpstreamDown) && !payments.wants(Kind::UpstreamUp));
        assert!(payments.covers(Some("pay.example.com")));
        assert!(payments.covers(Some("db.payments.internal")));
        assert!(!payments.covers(Some("shop.example.com")));
        // Events of the store, an instance or the configuration aren't about a host
        assert!(payments.covers(None));
        // A test is always sent
        assert!(payments.wants(Kind::Test));
    }

    #[test]
    fn test_blocks_are_checked() {
        let mut bad = block(&[], &[]);
        bad.url = "hooks.example.com".into();
        assert!(bad.check("ops").unwrap_err().contains("https://"));
        let mut bad = block(&[], &["api.*.com"]);
        assert!(bad.check("ops").unwrap_err().contains("api.*.com"));
        bad.hosts = vec![];
        bad.headers.insert("bad header".into(), "x".into());
        assert!(bad.check("ops").unwrap_err().contains("bad header"));
        assert!(block(&[], &["*", "*.example.com"]).check("ops").is_ok());
    }
}
