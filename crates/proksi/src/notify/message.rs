//! What a webhook receives: a sentence saying what happened, as JSON, or as a Slack or Discord
//! message

use crate::config::notify::{Format, Kind};

use super::Event;

/// What happened, in a sentence
pub fn summary(event: &Event) -> String {
    let host = event.host.as_deref().unwrap_or("?");
    let upstream = event.upstream.as_deref().unwrap_or("?");
    let detail = event
        .detail
        .as_deref()
        .map(|detail| format!(": {detail}"))
        .unwrap_or_default();
    // An upstream of a location, or of a `tcp` block
    let of = match (&event.tcp, &event.location) {
        (Some(block), _) => format!("tcp block {block}"),
        (None, Some(location)) => format!("{host} {location}"),
        (None, None) => host.to_string(),
    };
    match event.kind {
        Kind::UpstreamDown => format!("Upstream {upstream} of {of} is down{detail}"),
        Kind::UpstreamUp => format!("Upstream {upstream} of {of} is up again"),
        Kind::CertificateIssued => format!("Certificate issued for {host}"),
        Kind::CertificateRenewed => format!("Certificate renewed for {host}"),
        Kind::CertificateFailed => format!("Certificate for {host} could not be obtained{detail}"),
        Kind::CertificateExpiring => match event.days_left {
            Some(days) if days < 0 => format!("Certificate for {host} expired"),
            Some(days) => format!("Certificate for {host} expires in {days} days"),
            None => format!("Certificate for {host} expires soon"),
        },
        Kind::ConfigReloaded => format!("Configuration reloaded{detail}"),
        Kind::ConfigReloadFailed => {
            format!("Configuration change refused, the previous one is kept{detail}")
        }
        Kind::HostChanged => {
            let by = event
                .author
                .as_deref()
                .map(|author| format!(" by {author}"))
                .unwrap_or_default();
            let change = event.detail.as_deref().unwrap_or("changed");
            match event.version {
                Some(version) => format!("Host {host} {change}{by} (version {version})"),
                None => format!("Host {host} {change}{by}"),
            }
        }
        Kind::StoreDown => format!("The store stopped answering{detail}"),
        Kind::StoreUp => "The store answers again".to_string(),
        Kind::InstanceStarted => "Proksi started".to_string(),
        Kind::InstanceStopping => format!("Proksi is shutting down{detail}"),
        Kind::Test => "A test from Proksi: this webhook works".to_string(),
    }
}

/// A sign of how it went, at the start of chat messages
fn sign(kind: Kind) -> &'static str {
    match kind {
        Kind::UpstreamDown
        | Kind::CertificateFailed
        | Kind::ConfigReloadFailed
        | Kind::StoreDown => "🔴",
        Kind::CertificateExpiring | Kind::InstanceStopping => "🟠",
        Kind::UpstreamUp | Kind::StoreUp | Kind::CertificateIssued | Kind::CertificateRenewed => {
            "🟢"
        }
        Kind::ConfigReloaded | Kind::HostChanged | Kind::InstanceStarted | Kind::Test => "🔵",
    }
}

/// The body of the request to a webhook, as `format` says, from `instance`
pub fn body(event: &Event, format: Format, instance: &str) -> serde_json::Value {
    let summary = summary(event);
    match format {
        Format::Json => {
            let mut body = serde_json::json!({
                "event": event.kind.name(),
                "summary": summary,
                "instance": instance,
                "at": time::OffsetDateTime::from(event.at)
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
            });
            // Only what's known about it
            let fields = [
                ("host", event.host.clone().map(Into::into)),
                ("location", event.location.clone().map(Into::into)),
                ("upstream", event.upstream.clone().map(Into::into)),
                ("tcp", event.tcp.clone().map(Into::into)),
                ("detail", event.detail.clone().map(Into::into)),
                ("author", event.author.clone().map(Into::into)),
                ("version", event.version.map(Into::into)),
                ("days_left", event.days_left.map(Into::into)),
            ];
            let known = fields.into_iter().filter_map(
                |(name, value): (&str, Option<serde_json::Value>)| Some((name.to_string(), value?)),
            );
            if let Some(object) = body.as_object_mut() {
                object.extend(known);
            }
            body
        }
        Format::Slack => {
            serde_json::json!({ "text": format!("{} {summary} · {instance}", sign(event.kind)) })
        }
        Format::Discord => {
            serde_json::json!({ "content": format!("{} {summary} · {instance}", sign(event.kind)) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{body, summary};
    use crate::{
        config::notify::{Format, Kind},
        notify::Event,
    };

    #[test]
    fn test_summaries() {
        let down = Event {
            host: Some("api.example.com".into()),
            location: Some("/v1".into()),
            upstream: Some("10.0.0.1:80".into()),
            ..Event::new(Kind::UpstreamDown)
        };
        assert_eq!(
            summary(&down),
            "Upstream 10.0.0.1:80 of api.example.com /v1 is down"
        );
        let tcp = Event {
            tcp: Some("redis".into()),
            upstream: Some("10.0.0.5:6379".into()),
            ..Event::new(Kind::UpstreamUp)
        };
        assert_eq!(
            summary(&tcp),
            "Upstream 10.0.0.5:6379 of tcp block redis is up again"
        );
        let changed = Event {
            host: Some("api.example.com".into()),
            detail: Some("rolled back".into()),
            author: Some("ops".into()),
            version: Some(6),
            ..Event::new(Kind::HostChanged)
        };
        assert_eq!(
            summary(&changed),
            "Host api.example.com rolled back by ops (version 6)"
        );
        let expiring = Event {
            host: Some("api.example.com".into()),
            days_left: Some(5),
            ..Event::new(Kind::CertificateExpiring)
        };
        assert_eq!(
            summary(&expiring),
            "Certificate for api.example.com expires in 5 days"
        );
    }

    #[test]
    fn test_bodies() {
        let event = Event {
            host: Some("api.example.com".into()),
            detail: Some("rate limited".into()),
            ..Event::new(Kind::CertificateFailed)
        };
        let json = body(&event, Format::Json, "proksi-1");
        assert_eq!(json["event"], "certificate_failed");
        assert_eq!(json["host"], "api.example.com");
        assert_eq!(json["instance"], "proksi-1");
        assert_eq!(
            json["summary"],
            "Certificate for api.example.com could not be obtained: rate limited"
        );
        // Only what's known
        assert!(json.get("upstream").is_none());
        assert!(json["at"].as_str().is_some_and(|at| at.ends_with('Z')));

        let slack = body(&event, Format::Slack, "proksi-1");
        assert_eq!(
            slack["text"],
            "🔴 Certificate for api.example.com could not be obtained: rate limited · proksi-1"
        );
        let discord = body(&event, Format::Discord, "proksi-1");
        assert!(discord["content"]
            .as_str()
            .is_some_and(|content| content.starts_with("🔴 ")));
    }
}
