//! Request mirroring: copies of a location's requests, sent to upstreams of their own as the
//! requests are proxied. A copy is made of the request as sent to the upstream, and sent in the
//! background once its body has been proxied. Its answer is dropped: requests never wait on
//! their copies, nor fail with them. Copies over `max_in_flight` are dropped.

use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use bytes::Bytes;
use once_cell::sync::Lazy;
use pingora::{
    connectors::http::Connector,
    http::RequestHeader,
    lb::{selection::RoundRobin, LoadBalancer},
    upstreams::peer::HttpPeer,
};

use crate::config::{host::HealthCheck, RouteUpstream};

/// A location's mirror, ready to take copies
pub struct Mirror {
    percent: u8,
    max_body_size: u64,
    max_in_flight: usize,
    timeout: Duration,
    pub load_balancer: Arc<LoadBalancer<RoundRobin>>,
    upstreams: Vec<RouteUpstream>,
    /// The index of its first upstream among the location's
    first: usize,
    in_flight: AtomicUsize,
    pub counts: Counts,
}

/// What became of the copies
#[derive(Default)]
pub struct Counts {
    /// Answered by the mirror (whatever the answer)
    pub sent: AtomicU64,
    /// Not answered: no upstream up, an error, or out of time
    pub failed: AtomicU64,
    /// Not sent: `max_in_flight` were being sent
    pub dropped: AtomicU64,
    /// Not sent: the body was larger than `max_body_size`
    pub too_large: AtomicU64,
}

impl Mirror {
    /// The mirror of a location, whose upstreams are numbered from `first` among the location's
    pub fn build(
        mirror: &crate::config::host::Mirror,
        first: usize,
        health_check: Option<HealthCheck>,
        watched: crate::routing::build::Watched,
    ) -> Self {
        let mut load_balancer = crate::routing::build::load_balancer_at(&mirror.upstreams, first);
        crate::routing::build::set_health_check(&mut load_balancer, health_check, watched);
        Self {
            percent: mirror.percent,
            max_body_size: mirror.max_body_size,
            max_in_flight: mirror.max_in_flight,
            timeout: Duration::from_secs(mirror.timeout_secs),
            load_balancer: Arc::new(load_balancer),
            upstreams: mirror.upstreams.clone(),
            first,
            in_flight: AtomicUsize::new(0),
            counts: Counts::default(),
        }
    }

    /// The share of requests copied
    pub fn percent(&self) -> u8 {
        self.percent
    }

    /// Whether it holds the upstream at `index` among the location's
    pub fn holds(&self, index: usize) -> bool {
        (self.first..self.first + self.upstreams.len()).contains(&index)
    }

    /// Whether a request is copied: as a rule said (`then { mirror = ... }`), else by its share
    fn copies(&self, ruled: Option<bool>) -> bool {
        ruled.unwrap_or_else(|| self.percent >= 100 || random_percent() < self.percent)
    }
}

/// Where the copy of a request is
#[derive(Default)]
pub enum State {
    #[default]
    Undecided,
    /// Not copied
    Off,
    /// Its request, and its body so far
    Copying(Box<Copy>),
    Sent,
}

pub struct Copy {
    mirror: Arc<Mirror>,
    request: Box<RequestHeader>,
    body: Vec<Bytes>,
    size: u64,
}

/// The request is sent to the upstream (again, on a retry): copied as sent, without the headers
/// of the upstream it went to (`added`), if the location mirrors it
pub fn start<'a>(
    state: &mut State,
    mirror: Option<&Arc<Mirror>>,
    ruled: Option<bool>,
    request: &RequestHeader,
    added: impl Iterator<Item = &'a str>,
) {
    let Some(mirror) = mirror else {
        return;
    };
    match state {
        State::Undecided if !mirror.copies(ruled) => {
            *state = State::Off;
            return;
        }
        // Copying already: a retry, copied again as sent now
        State::Undecided | State::Copying(_) => {}
        State::Off | State::Sent => return,
    }
    let mut request = Box::new(request.clone());
    for name in added {
        request.remove_header(name);
    }
    *state = State::Copying(Box::new(Copy {
        mirror: mirror.clone(),
        request,
        body: vec![],
        size: 0,
    }));
}

/// Each part of the request body, as it's proxied: kept for the copy, sent once it all was
pub fn body(state: &mut State, part: Option<&Bytes>, end: bool) {
    let State::Copying(copy) = state else {
        return;
    };
    if let Some(part) = part.filter(|part| !part.is_empty()) {
        copy.size += part.len() as u64;
        if copy.size > copy.mirror.max_body_size {
            copy.mirror.counts.too_large.fetch_add(1, Ordering::Relaxed);
            *state = State::Off;
            return;
        }
        // The same bytes: counted, not copied
        copy.body.push(part.clone());
    }
    if end {
        if let State::Copying(copy) = std::mem::replace(state, State::Sent) {
            copy.send();
        }
    }
}

/// Copies are sent over connections of their own, reused between copies
static CONNECTOR: Lazy<Connector> = Lazy::new(|| Connector::new(None));

impl Copy {
    /// Sends the copy in the background, unless `max_in_flight` are being sent
    fn send(self: Box<Self>) {
        let mirror = self.mirror.clone();
        if mirror.in_flight.fetch_add(1, Ordering::Relaxed) >= mirror.max_in_flight {
            mirror.in_flight.fetch_sub(1, Ordering::Relaxed);
            mirror.counts.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        tokio::spawn(async move {
            match tokio::time::timeout(mirror.timeout, self.deliver()).await {
                Ok(Ok(())) => mirror.counts.sent.fetch_add(1, Ordering::Relaxed),
                Ok(Err(err)) => {
                    tracing::debug!("mirror: {err}");
                    mirror.counts.failed.fetch_add(1, Ordering::Relaxed)
                }
                Err(_) => {
                    tracing::debug!("mirror: no answer in {:?}", mirror.timeout);
                    mirror.counts.failed.fetch_add(1, Ordering::Relaxed)
                }
            };
            mirror.in_flight.fetch_sub(1, Ordering::Relaxed);
        });
    }

    async fn deliver(self: Box<Self>) -> pingora::Result<()> {
        let Copy {
            mirror,
            mut request,
            body,
            ..
        } = *self;
        let backend = mirror.load_balancer.select(b"", 32).ok_or_else(|| {
            pingora::Error::explain(pingora::ErrorType::ConnectNoRoute, "no upstream up")
        })?;
        let port = backend.addr.as_inet().map_or(0, std::net::SocketAddr::port);
        let upstream = crate::routing::build::upstream_index(&backend)
            .and_then(|index| index.checked_sub(mirror.first))
            .and_then(|index| mirror.upstreams.get(index));

        // The headers of the mirror's upstream
        let added = upstream.and_then(|upstream| upstream.headers.as_ref()?.add.as_ref());
        for header in added.into_iter().flatten() {
            request
                .insert_header(header.name.to_string(), header.value.as_ref())
                .ok();
        }

        let sni = upstream
            .and_then(|upstream| upstream.sni.clone())
            .unwrap_or_default();
        let mut peer = HttpPeer::new(backend, port == 443, sni);
        peer.options = crate::proxy_server::default_peer_opts(
            crate::proxy_server::timeouts::upstream_timeouts(),
        );

        let (mut session, _) = CONNECTOR.get_http_session(&peer).await?;
        session.write_request_header(request).await?;
        let parts = body.len();
        for (index, part) in body.into_iter().enumerate() {
            session.write_request_body(part, index + 1 == parts).await?;
        }
        session.finish_request_body().await?;
        session.read_response_header().await?;
        while session.read_response_body().await?.is_some() {}
        CONNECTOR
            .release_http_session(session, &peer, peer.options.idle_timeout)
            .await;
        Ok(())
    }
}

/// A number from 0 to 99, from a generator of each thread (xorshift64*)
fn random_percent() -> u8 {
    thread_local! {
        static STATE: Cell<u64> = Cell::new(uuid::Uuid::new_v4().as_u64_pair().0 | 1);
    }
    STATE.with(|state| {
        let mut x = state.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.set(x);
        u8::try_from(x.wrapping_mul(0x2545_f491_4f6c_dd1d) % 100).unwrap_or(0)
    })
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use pingora::http::RequestHeader;

    use super::{body, start, Mirror, State};
    use crate::config::{host, RouteUpstream};

    fn mirror(percent: u8, max_body_size: u64) -> std::sync::Arc<Mirror> {
        std::sync::Arc::new(Mirror::build(
            &host::Mirror {
                percent,
                max_body_size,
                max_in_flight: 1,
                timeout_secs: 1,
                upstreams: vec![RouteUpstream {
                    ip: "127.0.0.1".into(),
                    port: 1,
                    ..RouteUpstream::default()
                }],
            },
            2,
            None,
            crate::routing::build::Watched::default(),
        ))
    }

    fn request() -> RequestHeader {
        let mut request = RequestHeader::build("POST", b"/orders", None).unwrap();
        request.insert_header("x-upstream", "primary").unwrap();
        request.insert_header("x-client", "1").unwrap();
        request
    }

    fn copied(state: &State) -> Option<&super::Copy> {
        match state {
            State::Copying(copy) => Some(copy),
            _ => None,
        }
    }

    #[test]
    fn test_requests_copied_by_share_and_by_rules() {
        let all = mirror(100, 1024);
        let none = mirror(0, 1024);
        let decided = |mirror: &std::sync::Arc<Mirror>, ruled: Option<bool>| {
            let mut state = State::default();
            start(&mut state, Some(mirror), ruled, &request(), std::iter::empty());
            copied(&state).is_some()
        };
        assert!(decided(&all, None));
        assert!(!decided(&none, None));
        // Rules choose whatever the share
        assert!(decided(&none, Some(true)));
        assert!(!decided(&all, Some(false)));

        // Roughly its share
        let half = mirror(50, 1024);
        let copies = (0..2000).filter(|_| decided(&half, None)).count();
        assert!((800..1200).contains(&copies), "{copies}");

        // Locations without a mirror copy nothing
        let mut state = State::default();
        start(&mut state, None, Some(true), &request(), std::iter::empty());
        assert!(matches!(state, State::Undecided));
    }

    #[test]
    fn test_the_copy_is_the_request_as_sent_without_the_upstream_s_headers() {
        let mut state = State::default();
        start(
            &mut state,
            Some(&mirror(100, 1024)),
            None,
            &request(),
            ["x-upstream"].into_iter(),
        );
        let copy = copied(&state).unwrap();
        assert_eq!(copy.request.uri.path(), "/orders");
        assert_eq!(copy.request.headers.get("x-client").unwrap(), "1");
        assert!(copy.request.headers.get("x-upstream").is_none());
    }

    #[test]
    fn test_bodies_are_kept_up_to_their_limit() {
        let mirror = mirror(100, 8);
        let mut state = State::default();
        start(
            &mut state,
            Some(&mirror),
            None,
            &request(),
            std::iter::empty(),
        );
        body(&mut state, Some(&Bytes::from_static(b"1234")), false);
        body(&mut state, Some(&Bytes::from_static(b"5678")), false);
        let copy = copied(&state).unwrap();
        assert_eq!((copy.body.len(), copy.size), (2, 8));

        // One more byte: not copied
        body(&mut state, Some(&Bytes::from_static(b"9")), false);
        assert!(matches!(state, State::Off));
        assert_eq!(
            mirror.counts.too_large.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn test_retries_are_copied_again() {
        let mirror = mirror(100, 1024);
        let mut state = State::default();
        start(&mut state, Some(&mirror), None, &request(), std::iter::empty());
        body(&mut state, Some(&Bytes::from_static(b"part")), false);
        // Sent to an upstream again: its body is proxied again too
        start(&mut state, Some(&mirror), None, &request(), std::iter::empty());
        assert_eq!(copied(&state).unwrap().size, 0);
    }

    #[test]
    fn test_upstreams_are_numbered_after_the_location_s() {
        let mirror = mirror(100, 1024);
        assert!(mirror.holds(2) && !mirror.holds(1) && !mirror.holds(3));
    }
}
