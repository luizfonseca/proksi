//! Pingora logs every failed TLS handshake with a client as an error ("Downstream handshake
//! error ... unexpected EOF"): scanners, load balancers' TCP health checks and clients giving up
//! make them continuous, though nothing is wrong with proksi. They're counted instead
//! (`proksi_tls_handshake_failures_total`), and logged only at the `debug` level.

use std::fmt;

use tracing::{field::Field, Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const TARGET: &str = "pingora_core::services::listening";

/// Leaves out the handshake failures Pingora logs, unless `debug` is set
pub struct HandshakeErrorFilter {
    pub debug: bool,
}

impl<S: Subscriber> Layer<S> for HandshakeErrorFilter {
    fn event_enabled(&self, event: &Event<'_>, _: Context<'_, S>) -> bool {
        if self.debug {
            return true;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        // Pingora logs through `log`: its target is a field of the event
        let target = fields
            .log_target
            .as_deref()
            .unwrap_or(event.metadata().target());
        !(target == TARGET
            && (fields.message.starts_with("Downstream handshake error")
                || fields.message.starts_with("Downstream handshake timeout")))
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    log_target: Option<String>,
}

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            "log.target" => self.log_target = Some(value.to_string()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use tracing_subscriber::{layer::SubscriberExt, Layer};

    use super::HandshakeErrorFilter;

    /// Counts the events that get through
    struct Count(Arc<AtomicUsize>);

    impl<S: tracing::Subscriber> Layer<S> for Count {
        fn on_event(&self, _: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn logged(debug: bool, log: impl FnOnce()) -> usize {
        let count = Arc::new(AtomicUsize::new(0));
        let subscriber = tracing_subscriber::registry()
            .with(Count(count.clone()))
            .with(HandshakeErrorFilter { debug });
        tracing::subscriber::with_default(subscriber, log);
        count.load(Ordering::Relaxed)
    }

    #[test]
    fn test_failed_handshakes_are_left_out() {
        let handshake = || {
            tracing::error!(
                target: "pingora_core::services::listening",
                "Downstream handshake error from 10.42.0.1:23327:  TLSHandshakeFailure context: TLS accept() failed: unexpected EOF"
            );
            tracing::error!(target: "pingora_core::services::listening", "Downstream handshake timeout");
        };
        assert_eq!(logged(false, handshake), 0);
        assert_eq!(logged(true, handshake), 2);
    }

    #[test]
    fn test_other_errors_are_logged() {
        let others = || {
            tracing::error!(target: "pingora_core::services::listening", "Accept() failed: Too many open files");
            tracing::error!("Downstream handshake error, from proksi itself");
        };
        assert_eq!(logged(false, others), 2);
    }
}
