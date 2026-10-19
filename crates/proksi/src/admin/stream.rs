//! Server-sent events: the stats every second, and the requests being served

use std::time::Duration;

use bytes::Bytes;
use http::{header, StatusCode};
use pingora::{protocols::http::ServerSession, server::ShutdownWatch};
use tokio::sync::broadcast::error::RecvError;

use crate::metrics::requests;


/// Starts the events; returns whether the headers were written
async fn start(session: &mut ServerSession) -> bool {
    let Some(mut header) = super::response_header(StatusCode::OK, "text/event-stream") else {
        return false;
    };
    header
        .insert_header(header::TRANSFER_ENCODING, "chunked")
        .is_ok()
        && session
            .write_response_header(Box::new(header))
            .await
            .is_ok()
}

async fn send(session: &mut ServerSession, event: String) -> bool {
    session
        .write_response_body(Bytes::from(event), false)
        .await
        .is_ok()
}

/// The stats over `window`, every second, until the client leaves or Proksi shuts down
pub async fn stats(mut session: ServerSession, window: Duration, mut shutdown: ShutdownWatch) {
    if !start(&mut session).await {
        return;
    }
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let event = format!("data: {}\n\n", super::stats(window));
                if !send(&mut session, event).await {
                    return;
                }
            }
            _ = shutdown.changed() => break,
        }
    }
    let _ = session.write_response_body(Bytes::new(), true).await;
}

/// Each request served (`data:`), of `host` if given, at most 100 a second; once a second, how
/// many weren't sent (`event: skipped`)
pub async fn requests(
    mut session: ServerSession,
    host: Option<String>,
    mut shutdown: ShutdownWatch,
) {
    if !start(&mut session).await {
        return;
    }
    let mut watch = requests::watch(host.clone());
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let (mut sent, mut skipped) = (0u64, 0u64);
    loop {
        tokio::select! {
            received = watch.receiver.recv() => {
                let served = match received {
                    Ok(served) => served,
                    Err(RecvError::Lagged(count)) => {
                        skipped += count;
                        continue;
                    }
                    Err(RecvError::Closed) => break,
                };
                if host.as_ref().is_some_and(|host| !served.host.eq_ignore_ascii_case(host)) {
                    continue;
                }
                if sent >= requests::PER_SECOND {
                    skipped += 1;
                    continue;
                }
                sent += 1;
                let Ok(json) = serde_json::to_string(served.as_ref()) else {
                    continue;
                };
                if !send(&mut session, format!("data: {json}\n\n")).await {
                    return;
                }
            }
            _ = interval.tick() => {
                requests::new_second();
                // Also keeps the connection open while nothing is served
                let event = format!("event: skipped\ndata: {skipped}\n\n");
                if !send(&mut session, event).await {
                    return;
                }
                (sent, skipped) = (0, 0);
            }
            _ = shutdown.changed() => break,
        }
    }
    let _ = session.write_response_body(Bytes::new(), true).await;
}
