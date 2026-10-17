//! TLS handshakes with clients that start and never complete
//! (`proksi_tls_handshake_failures_total`): each one carries a marker from its `ClientHello`,
//! counted as a failure when the connection is dropped before the handshake completed

use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use openssl::{
    ex_data::Index,
    ssl::{Ssl, SslRef},
};

/// Where the marker is kept on each connection (none if OpenSSL has no room for it)
static MARKER: Lazy<Option<Index<Ssl, Handshake>>> = Lazy::new(|| Ssl::new_ex_index().ok());

#[derive(Default)]
struct Handshake {
    completed: AtomicBool,
}

impl Drop for Handshake {
    fn drop(&mut self) {
        if !self.completed.load(Ordering::Relaxed) {
            crate::metrics::tls_handshake_failed();
        }
    }
}

/// A client sent its `ClientHello`
pub fn started(ssl: &mut SslRef) {
    if let Some(index) = *MARKER {
        ssl.set_ex_data(index, Handshake::default());
    }
}

/// The handshake completed
pub fn completed(ssl: &SslRef) {
    if let Some(handshake) = MARKER.and_then(|index| ssl.ex_data(index)) {
        handshake.completed.store(true, Ordering::Relaxed);
    }
}
