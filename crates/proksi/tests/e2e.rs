//! End-to-end tests running the `proksi` binary against local upstreams.
//!
//! See `harness/mod.rs` for how proksi and the upstreams are started.

mod harness;

use futures_util::future::join_all;
use harness::{websocket_echo, Harness, Route, Upstream, UTF8_CONTENT_DISPOSITION};

#[tokio::test(flavor = "multi_thread")]
async fn test_proxies_requests_to_upstream() {
    let mut proksi = Harness::start(vec![Route::new("app.test", Upstream::Http)]).await;

    let res = proksi.get("app.test", "/hello?x=1").await.unwrap();
    assert_eq!(res.status, 200);

    let upstream_request = res.upstream_request();
    assert_eq!(upstream_request["path"], "/hello?x=1");
    assert_eq!(upstream_request["method"], "GET");
    // The host header is forwarded as is (including the port used by the client)
    let host = upstream_request["headers"]["host"].as_str().unwrap();
    assert!(host.starts_with("app.test:"), "{host}");
    assert_eq!(upstream_request["headers"]["x-custom"], "e2e");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_host_header_is_case_insensitive() {
    let mut proksi = Harness::start(vec![Route::new("app.test", Upstream::Http)]).await;

    let res = proksi
        .client()
        .get(format!("https://app.test:{}/", proksi.https_port))
        .header("host", "App.Test")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_unmatched_path_returns_404_and_unknown_host_is_rejected() {
    let mut proksi =
        Harness::start(vec![Route::new("app.test", Upstream::Http).paths(&["/api/*"])]).await;

    let res = proksi.get("app.test", "/api/users").await.unwrap();
    assert_eq!(res.status, 200);

    let res = proksi.get("app.test", "/other").await.unwrap();
    assert_eq!(res.status, 404);

    // There's no certificate for unknown hosts, so the TLS handshake fails
    assert!(proksi.get("unknown.test", "/").await.is_err());

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_http_port_redirects_to_https() {
    let mut proksi = Harness::start(vec![Route::new("app.test", Upstream::Http)]).await;

    let res = proksi.get_http_port("app.test", "/path?x=1").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");
    assert!(
        res.to_lowercase()
            .contains("location: https://app.test/path?x=1"),
        "{res}"
    );

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_http_port_rejects_invalid_host() {
    let mut proksi = Harness::start(vec![Route::new("app.test", Upstream::Http)]).await;

    let res = proksi.get_http_port("bad host", "/").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 400"), "{res}");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_disk_cache_serves_hits() {
    let mut proksi = Harness::start(vec![Route::new("cache.test", Upstream::Http).cache()]).await;

    let first = proksi.get("cache.test", "/asset.js?v=1").await.unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(first.header("cache-status"), Some("fwd=miss"));

    let second = proksi.get("cache.test", "/asset.js?v=1").await.unwrap();
    assert_eq!(second.status, 200);
    assert_eq!(second.header("cache-status"), Some("hit"));
    assert_eq!(second.body, first.body);

    assert_eq!(proksi.http_upstream.hits("/asset.js?v=1"), 1);
    assert!(proksi.cache_dir().join("cache.test").is_dir());

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_disk_cache_keeps_non_ascii_headers() {
    let mut proksi = Harness::start(vec![Route::new("cache.test", Upstream::Http).cache()]).await;

    for cache_status in ["fwd=miss", "hit"] {
        let res = proksi.get("cache.test", "/utf8/file.pdf").await.unwrap();
        assert_eq!(res.status, 200);
        assert_eq!(res.header("cache-status"), Some(cache_status));
        assert_eq!(
            res.headers["content-disposition"].as_bytes(),
            UTF8_CONTENT_DISPOSITION.as_bytes()
        );
    }

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_upstream_with_hostname() {
    let mut proksi =
        Harness::start(vec![
            Route::new("app.test", Upstream::Http).upstream_ip("localhost")
        ])
        .await;

    // `localhost` may also resolve to ::1, where the upstream isn't listening,
    // so only check that a request eventually succeeds
    let mut statuses = vec![];
    for _ in 0..4 {
        statuses.push(proksi.get("app.test", "/").await.unwrap().status);
    }
    assert!(statuses.contains(&200), "{statuses:?}");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_websocket_echo() {
    let mut proksi = Harness::start(vec![Route::new("ws.test", Upstream::WebSocket)]).await;

    let mut ws = proksi.websocket("ws.test", "/ws").await.unwrap();
    for message in ["hello", "world"] {
        assert_eq!(websocket_echo(&mut ws, message).await.unwrap(), message);
    }

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_many_concurrent_websockets() {
    const CONNECTIONS: usize = 500;

    let mut proksi = Harness::start(vec![
        Route::new("ws.test", Upstream::WebSocket),
        Route::new("app.test", Upstream::Http),
    ])
    .await;

    // Open every connection at once and keep them all open
    let sockets = join_all(
        (0..CONNECTIONS).map(|i| proksi.websocket("ws.test", format!("/ws?id={i}"))),
    )
    .await;
    let (mut sockets, errors): (Vec<_>, Vec<_>) = sockets.into_iter().partition(Result::is_ok);
    assert!(
        errors.is_empty(),
        "{} of {CONNECTIONS} websockets failed to connect: {:?}\n{}",
        errors.len(),
        errors.iter().take(5).collect::<Vec<_>>(),
        proksi.error_logs()
    );

    let echoes = join_all(sockets.iter_mut().enumerate().map(|(i, ws)| async move {
        let message = format!("message {i}");
        let path = format!("/ws?id={i}");
        match websocket_echo(ws.as_mut().unwrap(), &message).await {
            Ok(echo) if echo == message => Ok(()),
            Ok(echo) => Err((path, format!("unexpected echo {echo:?}"))),
            Err(err) => Err((path, err)),
        }
    }))
    .await;
    // Give proksi a moment to write access logs for closed connections
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let errors = echoes
        .into_iter()
        .filter_map(Result::err)
        .map(|(path, err)| {
            format!(
                "{path}: {err}\n  upstream: {:?}\n  proksi: {}",
                proksi.websocket_upstream.event(&path),
                proksi.access_log(&path)
            )
        })
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "{} of {CONNECTIONS} websockets failed to echo: {:?}\n{}",
        errors.len(),
        errors.iter().take(5).collect::<Vec<_>>(),
        proksi.error_logs()
    );

    // Other routes keep working while the websockets are open
    let res = proksi.get("app.test", "/").await.unwrap();
    assert_eq!(res.status, 200);

    proksi.assert_healthy();
}
