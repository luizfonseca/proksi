//! End-to-end tests running the `proksi` binary against local upstreams.
//!
//! See `harness/mod.rs` for how proksi and the upstreams are started.

// Test code: an unexpected value fails the test, loudly
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod harness;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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
async fn test_unknown_names_are_refused_in_the_tls_handshake() {
    let mut proksi = Harness::start(vec![Route::new("app.test", Upstream::Http)]).await;
    let port = proksi.https_port;

    // A name no host serves, or none: refused before any certificate is looked for
    let refused = proksi.handshake_error(port, Some("unknown.test")).await;
    assert!(
        refused
            .as_deref()
            .is_some_and(|err| err.contains("unrecognized name")),
        "{refused:?}"
    );
    let refused = proksi.handshake_error(port, None).await;
    assert!(
        refused
            .as_deref()
            .is_some_and(|err| err.contains("unrecognized name")),
        "{refused:?}"
    );

    // The hosts', whatever their case
    assert_eq!(proksi.handshake_error(port, Some("app.test")).await, None);
    assert_eq!(proksi.handshake_error(port, Some("App.Test")).await, None);

    // Scanners asking for other names aren't worth a line in the logs
    assert!(
        !proksi.logs().contains("No certificate found"),
        "{}",
        proksi.logs()
    );
    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_unmatched_path_returns_404_and_unknown_host_is_rejected() {
    let mut proksi = Harness::start(vec![
        Route::new("app.test", Upstream::Http).paths(&["/api/*"])
    ])
    .await;

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
    let mut proksi = Harness::start(vec![
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
    let sockets =
        join_all((0..CONNECTIONS).map(|i| proksi.websocket("ws.test", format!("/ws?id={i}"))))
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

const AUTO_RELOAD: &str = "auto_reload {\n  enabled = true\n  interval_secs = 1\n}";

#[tokio::test(flavor = "multi_thread")]
async fn test_config_reload_applies_route_changes() {
    let mut proksi =
        Harness::start_with(vec![Route::new("a.test", Upstream::Http)], AUTO_RELOAD).await;
    assert_eq!(proksi.get("a.test", "/").await.unwrap().status, 200);

    proksi.rewrite_routes(vec![Route::new("b.test", Upstream::Http)]);

    assert_eq!(proksi.wait_for("b.test", "/", 200).await, Some(200));
    // No host serves a.test anymore: its handshakes are refused
    let mut refused = None;
    for _ in 0..50 {
        refused = proksi
            .handshake_error(proksi.https_port, Some("a.test"))
            .await;
        if refused.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        refused.is_some_and(|err| err.contains("unrecognized name")),
        "{}",
        proksi.logs()
    );
    // Applied without restarting
    assert_eq!(
        proksi
            .logs()
            .matches("starting config watcher service")
            .count(),
        1
    );
    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_invalid_config_reload_keeps_routes() {
    let mut proksi =
        Harness::start_with(vec![Route::new("a.test", Upstream::Http)], AUTO_RELOAD).await;

    proksi.write_config("routes = [ { this is not valid");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    assert_eq!(proksi.get("a.test", "/").await.unwrap().status, 200);
    assert!(
        proksi
            .logs()
            .contains("is invalid, keeping the current configuration"),
        "{}",
        proksi.logs()
    );
    proksi.assert_healthy();
}

/// Redis set in `PROKSI_TEST_REDIS_URL` (e.g. `redis://127.0.0.1:6379`) with a dedicated,
/// emptied database. `None` when it's not set.
fn test_redis(db: u8) -> Option<(String, redis::Connection)> {
    let Ok(url) = std::env::var("PROKSI_TEST_REDIS_URL") else {
        eprintln!("PROKSI_TEST_REDIS_URL not set, skipping redis test");
        return None;
    };
    let url = format!("{}/{db}", url.trim_end_matches('/'));
    let mut conn = redis::Client::open(url.as_str())
        .unwrap()
        .get_connection()
        .unwrap();
    let _: () = redis::cmd("FLUSHDB").query(&mut conn).unwrap();
    Some((url, conn))
}

#[tokio::test(flavor = "multi_thread")]
async fn test_store_routes_override_file_routes() {
    use redis::Commands;

    let Some((redis_url, mut redis)) = test_redis(11) else {
        return;
    };
    let mut proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        &format!(
            "store {{\n  store_type = \"redis\"\n  redis_url = \"{redis_url}\"\n  sync_interval_secs = 1\n}}"
        ),
    )
    .await;
    assert_eq!(proksi.get("app.test", "/other").await.unwrap().status, 200);

    // A route for the same host in the store, only matching /store/*
    let stored = serde_json::json!({
        "schema_version": 1,
        "route": {
            "host": "app.test",
            "upstreams": [{ "ip": "127.0.0.1", "port": proksi.http_upstream.port }],
            "match_with": { "path": { "patterns": ["/store/*"] } },
        },
    });
    let _: () = redis::pipe()
        .atomic()
        .set("proksi:route:app.test", stored.to_string())
        .sadd("proksi:routes", "app.test")
        .incr("proksi:revision", 1)
        .query(&mut redis)
        .unwrap();

    assert_eq!(proksi.wait_for("app.test", "/other", 404).await, Some(404));
    assert_eq!(
        proksi.get("app.test", "/store/x").await.unwrap().status,
        200
    );

    // Every replica reports the revision it applied
    let replicas: Vec<String> = redis.keys("proksi:replica:*").unwrap();
    assert_eq!(replicas.len(), 1);
    let replica: serde_json::Value =
        serde_json::from_str(&redis.get::<_, String>(&replicas[0]).unwrap()).unwrap();
    assert_eq!(replica["revision"], 1);

    // Removing it from the store falls back to the configuration file's route
    let _: () = redis::pipe()
        .atomic()
        .del("proksi:route:app.test")
        .srem("proksi:routes", "app.test")
        .incr("proksi:revision", 1)
        .query(&mut redis)
        .unwrap();
    assert_eq!(proksi.wait_for("app.test", "/other", 200).await, Some(200));

    proksi.assert_healthy();
}

#[cfg(feature = "store-sqlite")]
#[tokio::test(flavor = "multi_thread")]
async fn test_sqlite_store_routes() {
    let db_dir = tempfile::tempdir().unwrap();
    let db_path = db_dir.path().join("proksi.db");
    let mut proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        &format!(
            "store {{\n  store_type = \"sqlite\"\n  database_url = \"sqlite://{}?mode=rwc\"\n  sync_interval_secs = 1\n}}",
            db_path.to_string_lossy()
        ),
    )
    .await;
    assert_eq!(proksi.get("app.test", "/other").await.unwrap().status, 200);

    // Tables are created by proksi on startup
    let db = sqlx::SqlitePool::connect(&format!("sqlite://{}", db_path.to_string_lossy()))
        .await
        .unwrap();
    let stored = serde_json::json!({
        "schema_version": 1,
        "route": {
            "host": "app.test",
            "upstreams": [{ "ip": "127.0.0.1", "port": proksi.http_upstream.port }],
            "match_with": { "path": { "patterns": ["/store/*"] } },
        },
    });
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO proksi_routes (host, schema_version, data, updated_at) VALUES ('app.test', 1, ?, 0)")
        .bind(stored.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE proksi_meta SET value = value + 1 WHERE key = 'revision'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(proksi.wait_for("app.test", "/other", 404).await, Some(404));
    assert_eq!(
        proksi.get("app.test", "/store/x").await.unwrap().status,
        200
    );

    let mut tx = db.begin().await.unwrap();
    sqlx::query("DELETE FROM proksi_routes WHERE host = 'app.test'")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE proksi_meta SET value = value + 1 WHERE key = 'revision'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(proksi.wait_for("app.test", "/other", 200).await, Some(200));
    proksi.assert_healthy();
}

const HOST_WITH_LOCATIONS: &str = r#"
host "app.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/v1/session" {
    headers {
      response {
        add = { "x-location" = "session" }
      }
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }

  location "/" {
    headers {
      response {
        add = { "x-location" = "default" }
      }
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_host_blocks_choose_the_location_by_path() {
    // The harness also writes its readiness route with `routes = [...]`: both formats together
    let mut proksi = Harness::start_with(vec![], HOST_WITH_LOCATIONS).await;
    proksi.resolve("app.test");

    for (path, location) in [
        ("/", "default"),
        ("/other", "default"),
        ("/v1/session", "session"),
        ("/v1/session/", "session"),
        ("/v1/session/refresh", "session"),
        ("//v1/session", "session"),
        ("/v1/sessions", "default"),
    ] {
        let res = proksi.get("app.test", path).await.unwrap();
        assert_eq!(res.status, 200, "{path}");
        assert_eq!(res.header("x-location"), Some(location), "{path}");
        // Matching uses the normalized path, but the request is forwarded unchanged
        assert_eq!(res.upstream_request()["path"], path, "{path}");
    }

    proksi.assert_healthy();
}

const HOST_WITH_BLOCKS: &str = r#"
host "auth.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    basic_auth {
      user = "user"
      pass = "pass"
    }
    oauth2 {
      provider      = "github"
      client_id     = "client"
      client_secret = "secret"
      jwt_secret    = "jwt"
    }
  }

  location "/oauth-first" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    basic_auth {
      user = "user"
      pass = "pass"
    }
    oauth2 {
      provider      = "github"
      client_id     = "client"
      client_secret = "secret"
      jwt_secret    = "jwt"
      priority      = 10
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_blocks_run_by_priority() {
    let mut proksi = Harness::start_with(vec![], HOST_WITH_BLOCKS).await;
    proksi.resolve("auth.test");
    let url = |path: &str| format!("https://auth.test:{}{path}", proksi.https_port);

    // Same priority (20): basic_auth runs first, then oauth2
    let res = proksi.client().get(url("/")).send().await.unwrap();
    assert_eq!(res.status(), 401);

    let res = proksi
        .client()
        .get(url("/"))
        .basic_auth("user", Some("pass"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 307);

    // `priority = 10`: oauth2 runs before basic_auth
    let res = proksi
        .client()
        .get(url("/oauth-first"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 307);
    let location = res.headers()["location"].to_str().unwrap();
    assert!(location.starts_with("https://github.com/"), "{location}");

    proksi.assert_healthy();
}

const HOST_WITH_RATE_LIMIT: &str = r#"
host "limited.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/login" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    rate_limit {
      requests = 2
      per      = "1m"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_rate_limit() {
    let mut proksi = Harness::start_with(vec![], HOST_WITH_RATE_LIMIT).await;
    proksi.resolve("limited.test");

    for _ in 0..2 {
        assert_eq!(
            proksi.get("limited.test", "/login").await.unwrap().status,
            200
        );
    }
    let res = proksi.get("limited.test", "/login").await.unwrap();
    assert_eq!(res.status, 429);
    assert_eq!(res.header("retry-after"), Some("60"));

    // The limit belongs to its location
    assert_eq!(proksi.get("limited.test", "/").await.unwrap().status, 200);

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_rate_limit_behind_a_trusted_proxy() {
    // Requests come from 127.0.0.1, as if it were a load balancer in front of proksi
    let mut proksi = Harness::start_with_server(
        vec![],
        "trusted_proxies = [\"127.0.0.1\"]",
        HOST_WITH_RATE_LIMIT,
    )
    .await;
    let login = |forwarded_for: &str| {
        format!(
            "GET /login HTTP/1.1\r\nHost: limited.test\r\nX-Forwarded-For: {forwarded_for}\r\nConnection: close\r\n\r\n"
        )
    };
    let status = |res: std::io::Result<String>| {
        res.unwrap()
            .split(' ')
            .nth(1)
            .unwrap_or_default()
            .to_string()
    };

    // Each client has its own limit (2 requests)
    for client in ["203.0.113.1", "203.0.113.2"] {
        for _ in 0..2 {
            let res = proksi
                .tls_request(proksi.https_port, "limited.test", &login(client))
                .await;
            assert_eq!(status(res), "200", "{client}");
        }
    }
    let res = proksi
        .tls_request(proksi.https_port, "limited.test", &login("203.0.113.1"))
        .await;
    assert_eq!(status(res), "429");
    // A client can't pick another address in front of the proxy's
    let res = proksi
        .tls_request(
            proksi.https_port,
            "limited.test",
            &login("198.51.100.7, 203.0.113.2"),
        )
        .await;
    assert_eq!(status(res), "429");

    // The client in the access log
    let log = proksi.access_log("/login");
    assert!(log.contains("\"client_ip\":\"203.0.113."), "{log}");

    proksi.assert_healthy();
}

const HOST_WITH_BODY_LIMIT: &str = r#"
host "upload.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    body_limit {
      max = "1KiB"
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_body_limit() {
    let mut proksi = Harness::start_without_https(HOST_WITH_BODY_LIMIT).await;
    let port = proksi.http_port;
    let post = |path: &str, length: usize| {
        format!(
            "POST {path} HTTP/1.1\r\nHost: upload.test\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{}",
            "a".repeat(length)
        )
    };

    let res = proksi
        .raw_http_on(port, post("/small", 100).as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"body_length\":100"), "{res}");

    // Content-Length over the limit: answered without reaching the upstream
    let res = proksi
        .raw_http_on(port, post("/large", 2000).as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 413"), "{res}");
    assert_eq!(proksi.http_upstream.hits("/large"), 0);

    // A chunked body is counted as it's proxied
    let chunk = format!("258\r\n{}\r\n", "a".repeat(600));
    let request = format!(
        "POST /chunked HTTP/1.1\r\nHost: upload.test\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{chunk}{chunk}{chunk}0\r\n\r\n"
    );
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 413"), "{res}");

    proksi.assert_healthy();
}

const HOST_WITH_REQUEST_ID: &str = r#"
host "logged.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    request_id {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_access_logs_name_the_upstream_connection() {
    let mut proksi = Harness::start_without_https(HOST_WITH_REQUEST_ID).await;

    let mut request_ids = vec![];
    for path in ["/first", "/second"] {
        let res = proksi
            .get_http_on(proksi.http_port, "logged.test", path)
            .await
            .unwrap();
        assert!(res.starts_with("HTTP/1.1 200"), "{res}");
        let request_id = res
            .lines()
            .find_map(|line| line.to_lowercase().strip_prefix("x-request-id: ").map(str::to_string))
            .unwrap_or_else(|| panic!("no request id: {res}"));
        request_ids.push(request_id);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let upstream = format!("127.0.0.1:{}", proksi.http_upstream.port);
    for (path, request_id) in [("/first", &request_ids[0]), ("/second", &request_ids[1])] {
        let log = proksi.access_log(path);
        let log: serde_json::Value =
            serde_json::from_str(&log).unwrap_or_else(|_| panic!("{path}: {log}"));
        let fields = log.get("fields").unwrap_or(&log);
        assert_eq!(fields["upstream_addr"], upstream.as_str(), "{log}");
        // The connection may come from proksi's readiness checks
        assert!(
            ["true", "false"].contains(&fields["reused_connection"].as_str().unwrap_or("")),
            "{log}"
        );
        assert_eq!(fields["request_id"], request_id.as_str(), "{log}");
        assert_eq!(fields["x_forwarded_for"], "", "{log}");
    }

    // X-Forwarded-For is logged as sent, though proksi doesn't trust it
    let res = proksi
        .raw_http_on(
            proksi.http_port,
            b"GET /forwarded HTTP/1.1\r\nHost: logged.test\r\nX-Forwarded-For: 203.0.113.7, 198.51.100.2\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let log = proksi.access_log("/forwarded");
    let log: serde_json::Value = serde_json::from_str(&log).unwrap_or_else(|_| panic!("{log}"));
    let fields = log.get("fields").unwrap_or(&log);
    assert_eq!(fields["x_forwarded_for"], "203.0.113.7, 198.51.100.2", "{log}");
    assert!(fields["client_ip"].as_str().unwrap_or("").starts_with("127.0.0.1:"), "{log}");
    assert!(fields["remote_addr"].as_str().unwrap_or("").starts_with("127.0.0.1:"), "{log}");

    // Health checks aren't logged
    let res = proksi.get_http_on(proksi.http_port, "logged.test", "/ping").await.unwrap();
    assert!(res.ends_with("pong"), "{res}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(proksi.access_log("/ping"), "no access log");

    proksi.assert_healthy();
}

const HOST_WITH_REQUEST_ID_SUFFIX: &str = r#"
host "logged.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    request_id {
      suffix = "ams1"
    }
  }

  location "/correlated" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    request_id {
      header_name = "X-Correlation-Id"
    }
  }
}
"#;

/// The `X-Request-Id` of a response to a request sending `id`, and the one its upstream got
async fn request_ids(proksi: &Harness, id: &str) -> (String, String) {
    let request = format!(
        "GET /id HTTP/1.1\r\nHost: logged.test\r\nX-Request-Id: {id}\r\nConnection: close\r\n\r\n"
    );
    let res = proksi.raw_http_on(proksi.http_port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let sent = res
        .lines()
        .find_map(|line| line.to_lowercase().strip_prefix("x-request-id: ").map(str::to_string))
        .unwrap_or_else(|| panic!("no request id: {res}"));
    let body = res.split("\r\n\r\n").nth(1).unwrap_or_default();
    let echoed: serde_json::Value =
        serde_json::from_str(body).unwrap_or_else(|_| panic!("{res}"));
    let received = echoed["headers"]["x-request-id"].as_str().unwrap_or_default().to_string();
    (sent, received)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_request_ids_from_clients_are_replaced() {
    let mut proksi = Harness::start_without_https(HOST_WITH_REQUEST_ID_SUFFIX).await;

    let (sent, received) = request_ids(&proksi, "chosen-by-the-client").await;
    assert_eq!(sent, received);
    assert!(sent.ends_with("-ams1"), "{sent}");
    assert_eq!(sent.len(), 36 + 5, "{sent}");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_request_ids_from_trusted_proxies_are_kept() {
    let mut proksi = Harness::start_with_server(
        vec![],
        "trusted_proxies = [\"127.0.0.1\"]",
        HOST_WITH_REQUEST_ID_SUFFIX,
    )
    .await;

    let (sent, received) = request_ids(&proksi, "edge-7f3a").await;
    assert_eq!((sent.as_str(), received.as_str()), ("edge-7f3a", "edge-7f3a"));

    // In a header of its own
    let request = b"GET /correlated HTTP/1.1\r\nHost: logged.test\r\nX-Correlation-Id: edge-9c1d\r\nConnection: close\r\n\r\n";
    let res = proksi.raw_http_on(proksi.http_port, request).await.unwrap();
    assert!(res.contains("X-Correlation-Id: edge-9c1d\r\n"), "{res}");
    assert!(res.contains("\"x-correlation-id\":\"edge-9c1d\""), "{res}");
    assert!(!res.to_lowercase().contains("x-request-id"), "{res}");

    // Too long to pass on: a new one
    let (sent, _) = request_ids(&proksi, &"a".repeat(201)).await;
    assert!(sent.ends_with("-ams1"), "{sent}");

    proksi.assert_healthy();
}

const HOST_WITH_EXACT_AND_REGEX_LOCATIONS: &str = r#"
host "located.test" {
  tls = false

  location "/" {
    exact = true
    respond {
      body = "home"
    }
  }

  location "/blog" {
    respond {
      body = "blog"
    }
  }

  location "^/blog/[a-z]{8}/(posts|drafts)$" {
    respond {
      body = "regex"
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_exact_and_regex_locations() {
    let mut proksi = Harness::start_without_https(HOST_WITH_EXACT_AND_REGEX_LOCATIONS).await;

    for (path, expected) in [
        ("/", "home"),
        ("/blog/abcdefgh/posts", "regex"),
        ("/blog/abcdefgh/drafts?page=2", "regex"),
        ("//blog/./abcdefgh/posts", "regex"),
        // Below the regex's end, or not eight letters: the path location
        ("/blog/abcdefgh/posts/1", "blog"),
        ("/blog/abc/posts", "blog"),
        ("/blog", "blog"),
    ] {
        let res = proksi.get_http_on(proksi.http_port, "located.test", path).await.unwrap();
        assert!(res.starts_with("HTTP/1.1 200") && res.ends_with(expected), "{path}: {res}");
    }

    // Below the exact `/`: no location
    let res = proksi.get_http_on(proksi.http_port, "located.test", "/other").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");

    proksi.assert_healthy();
}

const HOST_WITH_COMPRESSION: &str = r#"
host "compressed.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    compress {}
  }

  location "/zstd-only" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    compress {
      algorithms = ["zstd"]
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_compress() {
    let mut proksi = Harness::start_without_https(HOST_WITH_COMPRESSION).await;
    let get = |path: &str, accept: &str| {
        format!(
            "GET {path} HTTP/1.1\r\nHost: compressed.test\r\nAccept-Encoding: {accept}\r\nConnection: close\r\n\r\n"
        )
    };

    for (path, accept, expected) in [
        ("/", "gzip", Some("gzip")),
        ("/", "br", Some("br")),
        ("/", "zstd, gzip", Some("zstd")),
        // The best one the client accepts, whatever its order (as browsers send it)
        ("/", "gzip, deflate, br, zstd", Some("zstd")),
        ("/", "gzip, br", Some("br")),
        ("/", "zstd;q=0, gzip", Some("gzip")),
        ("/", "identity", None),
        // Only the algorithms the block allows
        ("/zstd-only", "gzip", None),
        ("/zstd-only", "gzip, zstd", Some("zstd")),
    ] {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", proksi.http_port))
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut stream, get(path, accept).as_bytes())
            .await
            .unwrap();
        let mut response = vec![];
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response)
            .await
            .unwrap();
        let res = String::from_utf8_lossy(&response);
        assert!(res.starts_with("HTTP/1.1 200"), "{path} {accept}: {res}");
        assert_eq!(header(&res, "content-encoding"), expected, "{path} {accept}: {res}");
    }

    proksi.assert_healthy();
}

const HOSTS_WITH_REDIRECTS: &str = r#"
host "www.moved.test" {
  tls = false

  location "/" {
    redirect {
      to     = "https://moved.test{uri}"
      status = 301
    }
  }
}

host "moved.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }

  location "/docs" {
    redirect {
      to = "https://docs.moved.test{path}?from={host}"
    }
  }

  location "/maintenance" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    respond {
      status       = 503
      body         = "Back soon"
      content_type = "text/plain"
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_redirect_and_respond() {
    let mut proksi = Harness::start_without_https(HOSTS_WITH_REDIRECTS).await;
    let port = proksi.http_port;

    let res = proksi.get_http_on(port, "www.moved.test", "/a/b?c=d").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 301"), "{res}");
    assert_eq!(header(&res, "location"), Some("https://moved.test/a/b?c=d"));

    let res = proksi.get_http_on(port, "moved.test", "/docs/intro?x=1").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 302"), "{res}");
    assert_eq!(
        header(&res, "location"),
        Some("https://docs.moved.test/docs/intro?from=moved.test")
    );

    let res = proksi.get_http_on(port, "moved.test", "/maintenance").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 503"), "{res}");
    assert_eq!(header(&res, "content-type"), Some("text/plain"));
    assert!(res.ends_with("Back soon"), "{res}");
    assert_eq!(proksi.http_upstream.hits("/maintenance"), 0);

    // Other locations are proxied
    let res = proksi.get_http_on(port, "moved.test", "/other").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    proksi.assert_healthy();
}

const HOST_WITH_IP_FILTERS: &str = r#"
host "filtered.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }

  location "/admin" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    ip_filter {
      allow = ["10.0.0.0/8", "203.0.113.9"]
      deny  = ["10.0.0.13"]
    }
  }

  location "/public" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    ip_filter {
      deny = ["198.51.100.0/24"]
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_ip_filter() {
    // Clients are found behind the trusted proxy (the test itself, on 127.0.0.1)
    let mut proksi = Harness::start_with_server_ready_over_http(
        "trusted_proxies = [\"127.0.0.1\"]",
        HOST_WITH_IP_FILTERS,
    )
    .await;
    let port = proksi.http_port;

    for (path, client, expected) in [
        ("/admin", "10.1.2.3", "200"),
        ("/admin", "203.0.113.9", "200"),
        // Denied, although its network is allowed
        ("/admin", "10.0.0.13", "403"),
        // Not allowed
        ("/admin", "198.51.100.7", "403"),
        ("/admin/users", "198.51.100.7", "403"),
        // Only a deny list: everyone else is allowed
        ("/public", "203.0.113.50", "200"),
        ("/public", "198.51.100.7", "403"),
        ("/other", "198.51.100.7", "200"),
    ] {
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: filtered.test\r\nX-Forwarded-For: {client}\r\nConnection: close\r\n\r\n"
        );
        let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
        assert_eq!(res.split(' ').nth(1), Some(expected), "{path} from {client}: {res}");
    }
    assert_eq!(proksi.http_upstream.hits("/admin/users"), 0);

    proksi.assert_healthy();
}

const HOST_WITH_CORS: &str = r#"
host "cors.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    cors {
      origins        = ["https://app.example.com", "https://*.preview.example.com"]
      methods        = ["GET", "POST"]
      expose_headers = ["X-Total-Count"]
      credentials    = true
      max_age_secs   = 600
    }

    basic_auth {
      user = "u"
      pass = "p"
    }
  }

  location "/public" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    cors {
      origins = ["*"]
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_cors() {
    let mut proksi = Harness::start_without_https(HOST_WITH_CORS).await;
    let port = proksi.http_port;
    let send = |request: String| {
        let proksi = &proksi;
        async move { proksi.raw_http_on(port, request.as_bytes()).await.unwrap() }
    };
    let preflight = |path: &str, origin: &str| {
        format!(
            "OPTIONS {path} HTTP/1.1\r\nHost: cors.test\r\nOrigin: {origin}\r\n\
             Access-Control-Request-Method: POST\r\n\
             Access-Control-Request-Headers: content-type, x-custom\r\nConnection: close\r\n\r\n"
        )
    };

    // Preflights are answered before authentication, without reaching the upstream
    let res = send(preflight("/items", "https://app.example.com")).await;
    assert!(res.starts_with("HTTP/1.1 204"), "{res}");
    assert_eq!(
        header(&res, "access-control-allow-origin"),
        Some("https://app.example.com")
    );
    assert_eq!(header(&res, "access-control-allow-methods"), Some("GET, POST"));
    assert_eq!(
        header(&res, "access-control-allow-headers"),
        Some("content-type, x-custom")
    );
    assert_eq!(header(&res, "access-control-allow-credentials"), Some("true"));
    assert_eq!(header(&res, "access-control-max-age"), Some("600"));
    assert_eq!(proksi.http_upstream.hits("/items"), 0);

    // A subdomain of the pattern
    let res = send(preflight("/items", "https://pr-12.preview.example.com")).await;
    assert_eq!(
        header(&res, "access-control-allow-origin"),
        Some("https://pr-12.preview.example.com")
    );

    // Other origins get no CORS headers
    for origin in ["https://evil.test", "https://preview.example.com", "http://app.example.com"] {
        let res = send(preflight("/items", origin)).await;
        assert!(res.starts_with("HTTP/1.1 204"), "{origin}: {res}");
        assert_eq!(header(&res, "access-control-allow-origin"), None, "{origin}: {res}");
    }

    // Requests get the headers on their response (here, from basic_auth)
    let res = send(
        "GET /items HTTP/1.1\r\nHost: cors.test\r\nOrigin: https://app.example.com\r\nConnection: close\r\n\r\n"
            .to_string(),
    )
    .await;
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(
        header(&res, "access-control-allow-origin"),
        Some("https://app.example.com")
    );
    assert_eq!(header(&res, "access-control-expose-headers"), Some("X-Total-Count"));
    assert_eq!(header(&res, "vary"), Some("Origin"));

    // Any origin, without credentials
    let res = send(
        "GET /public HTTP/1.1\r\nHost: cors.test\r\nOrigin: https://evil.test\r\nConnection: close\r\n\r\n"
            .to_string(),
    )
    .await;
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(header(&res, "access-control-allow-origin"), Some("*"));
    assert_eq!(header(&res, "access-control-allow-credentials"), None);

    proksi.assert_healthy();
}

const HOST_WITH_FORWARD_AUTH: &str = r#"
host "forward.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    forward_auth {
      url              = "http://127.0.0.1:{auth_port}/verify"
      response_headers = ["Remote-User"]
      timeout_secs     = 1
    }
  }

  location "/unreachable" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    forward_auth {
      url = "http://127.0.0.1:{closed_port}/verify"
    }
  }
}
"#;

/// An auth service: `200` with `Remote-User` for requests with the `session=ok` cookie, a
/// redirect to its login page otherwise. Slow for `/slow`. Keeps the requests it gets.
async fn auth_service() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(vec![]));
    let seen = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let Some((head, _)) = read_head(&mut stream).await else {
                    return;
                };
                let request = head.to_lowercase();
                seen.lock().unwrap().push(request.clone());
                if request.contains("x-forwarded-uri: /slow") {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                let response = if request.contains("cookie: session=ok") {
                    "HTTP/1.1 200 OK\r\nRemote-User: alice\r\nContent-Length: 0\r\n\r\n"
                } else {
                    "HTTP/1.1 302 Found\r\nLocation: https://auth.test/login\r\n\
                     Set-Cookie: flow=1\r\nContent-Length: 5\r\n\r\nlogin"
                };
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (port, requests)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_forward_auth() {
    let (auth_port, auth_requests) = auth_service().await;
    let closed_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = HOST_WITH_FORWARD_AUTH
        .replace("{auth_port}", &auth_port.to_string())
        .replace("{closed_port}", &closed_port.to_string());
    let mut proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let get = |path: &str, headers: &str| {
        format!("GET {path} HTTP/1.1\r\nHost: forward.test\r\n{headers}Connection: close\r\n\r\n")
    };

    // Signed in: proxied with the user the auth service answered (not the client's own)
    let request = get("/private?x=1", "Cookie: session=ok\r\nRemote-User: mallory\r\n");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let body = res.split("\r\n\r\n").nth(1).unwrap_or_default();
    let upstream: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(upstream["headers"]["remote-user"], "alice", "{res}");
    let checked = auth_requests.lock().unwrap().last().cloned().unwrap();
    for expected in [
        "get /verify http/1.1",
        "x-forwarded-method: get",
        "x-forwarded-proto: http",
        "x-forwarded-host: forward.test",
        "x-forwarded-uri: /private?x=1",
        "x-forwarded-for: 127.0.0.1",
        "cookie: session=ok",
    ] {
        assert!(checked.contains(expected), "{expected}: {checked}");
    }

    // Not signed in: the auth service's answer
    let res = proksi.raw_http_on(port, get("/private", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 302"), "{res}");
    assert_eq!(header(&res, "location"), Some("https://auth.test/login"));
    assert_eq!(header(&res, "set-cookie"), Some("flow=1"));
    assert!(res.ends_with("login"), "{res}");
    // Only the signed-in request reached the upstream
    assert_eq!(proksi.http_upstream.hits("/private?x=1"), 1);
    assert_eq!(proksi.http_upstream.hits("/private"), 0);

    // The auth service doesn't answer in time, or can't be reached
    let request = get("/slow", "Cookie: session=ok\r\n");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 502"), "{res}");
    let request = get("/unreachable", "Cookie: session=ok\r\n");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 502"), "{res}");
    assert_eq!(proksi.http_upstream.hits("/slow"), 0);

    proksi.assert_healthy();
}

const SERVER_RULES: &str = r#"
trusted_proxies = ["127.0.0.1"]

rule "old domain" {
  and {
    host { eq = "old.rules.test" }
  }
  then {
    redirect {
      to     = "https://rules.test{uri}"
      status = 301
    }
  }
}
"#;

const HOST_WITH_RULES: &str = r#"
host "rules.test" {
  tls = false

  rule "maintenance" {
    and {
      path { prefix = "/shop" }
      not {
        client { in = ["203.0.113.0/24"] }
      }
    }
    then {
      respond {
        status = 503
        body   = "Back soon"
      }
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    rule "crawlers" {
      and {
        header "user-agent" { matches = "(?i)(bytespider|petalbot)" }
      }
      then {
        respond { status = 403 }
      }
    }

    rule "beta" {
      and {
        cookie "beta" { eq = "1" }
        method { eq = "GET" }
      }
      then {
        redirect { to = "https://beta.rules.test{uri}" }
      }
    }

    rule "method override" {
      and {
        method { eq = "POST" }
        header "x-http-method-override" { eq = "DELETE" }
      }
      then {
        method { set = "DELETE" }
        headers {
          request {
            remove = ["x-http-method-override"]
            add    = { "x-overridden" = "1" }
          }
        }
      }
    }

    # Sees the request as the rule above changed it
    rule "deletes" {
      and {
        method { eq = "DELETE" }
      }
      then {
        request_id {}
        headers {
          response {
            add = { "x-rule" = "deletes" }
          }
        }
      }
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_rules() {
    let mut proksi =
        Harness::start_with_server_ready_over_http(SERVER_RULES, HOST_WITH_RULES).await;
    let port = proksi.http_port;
    let send = |request: String| {
        let proksi = &proksi;
        async move { proksi.raw_http_on(port, request.as_bytes()).await.unwrap() }
    };
    let request = |method: &str, host: &str, path: &str, headers: &str| {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{headers}Content-Length: 0\r\nConnection: close\r\n\r\n"
        )
    };
    let upstream = |res: &str| -> serde_json::Value {
        serde_json::from_str(res.split("\r\n\r\n").nth(1).unwrap_or_default()).unwrap_or_default()
    };

    // The server's rule, before any host: the old domain has no host of its own
    let res = send(request("GET", "old.rules.test", "/a?b=1", "")).await;
    assert!(res.starts_with("HTTP/1.1 301"), "{res}");
    assert_eq!(header(&res, "location"), Some("https://rules.test/a?b=1"));

    // The host's rule: maintenance, except for the office network
    let res = send(request("GET", "rules.test", "/shop/cart", "X-Forwarded-For: 198.51.100.7\r\n")).await;
    assert!(res.starts_with("HTTP/1.1 503"), "{res}");
    assert!(res.ends_with("Back soon"), "{res}");
    let res = send(request("GET", "rules.test", "/shop/cart", "X-Forwarded-For: 203.0.113.9\r\n")).await;
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    // The location's rules, in order: the first that answers ends the request
    let both = "User-Agent: Mozilla/5.0 (compatible; Bytespider)\r\nCookie: beta=1\r\n";
    let res = send(request("GET", "rules.test", "/", both)).await;
    assert!(res.starts_with("HTTP/1.1 403"), "{res}");
    let res = send(request("GET", "rules.test", "/page", "Cookie: a=b; beta=1\r\n")).await;
    assert!(res.starts_with("HTTP/1.1 302"), "{res}");
    assert_eq!(header(&res, "location"), Some("https://beta.rules.test/page"));

    // The method and headers changed for the upstream, and for the next rule
    let res = send(request("POST", "rules.test", "/items/1", "X-HTTP-Method-Override: DELETE\r\n")).await;
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let seen = upstream(&res);
    assert_eq!(seen["method"], "DELETE", "{res}");
    assert_eq!(seen["headers"]["x-overridden"], "1", "{res}");
    assert!(seen["headers"].get("x-http-method-override").is_none(), "{res}");
    // The blocks of a matched rule take part in the later stages: its request id, its headers
    assert!(seen["headers"]["x-request-id"].as_str().is_some_and(|id| !id.is_empty()), "{res}");
    assert_eq!(header(&res, "x-rule"), Some("deletes"));
    assert!(header(&res, "x-request-id").is_some(), "{res}");

    // No rule matches: proxied as it is
    let res = send(request("GET", "rules.test", "/page", "")).await;
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(header(&res, "x-rule"), None);

    proksi.assert_healthy();
}

const TCP_BLOCKS: &str = r#"
tcp "web" {
  listen = "127.0.0.1:{tcp_port_0}"
  upstream "127.0.0.1:{http_upstream_port}" {}
}

host "a.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  tcp "app" {
    listen = "127.0.0.1:{tcp_port_1}"
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}

host "b.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  # proksi's own HTTP port, which redirects b.test to HTTPS
  tcp "app" {
    listen = "127.0.0.1:{tcp_port_1}"
    upstream "127.0.0.1:{http_port}" {}
  }
}

host "pass.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }

  # TLS goes untouched to proksi's own HTTPS port, which serves the location above
  tcp "raw" {
    listen = "127.0.0.1:{tcp_port_2}"
    tls    = "passthrough"
    upstream "127.0.0.1:{https_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_tcp_blocks() {
    let mut proksi = Harness::start_with(vec![], TCP_BLOCKS).await;
    let [plain_port, tls_port, passthrough_port] = proksi.tcp_ports;
    let get = |host: &str, path: &str| {
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    };

    // Plain TCP
    let res = proksi
        .raw_http_on(plain_port, get("tcp.test", "/plain").as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/plain\""), "{res}");

    // TLS terminated with the certificate of the host, chosen by SNI
    let res = proksi
        .tls_request(tls_port, "a.test", &get("a.test", "/terminated"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/terminated\""), "{res}");

    let res = proksi
        .tls_request(tls_port, "b.test", &get("b.test", "/"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");

    // No block for other names
    let res = proksi
        .tls_request(tls_port, "c.test", &get("c.test", "/"))
        .await;
    assert!(res.as_deref().map_or(true, str::is_empty), "{res:?}");

    // TLS passthrough
    let res = proksi
        .tls_request(
            passthrough_port,
            "pass.test",
            &get("pass.test", "/passthrough"),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/passthrough\""), "{res}");

    // An access log line for each connection, once closed
    let logs = proksi.tcp_access_logs(3).await;
    let log = |block: &str| {
        logs.iter()
            .find(|log| log["block"] == block)
            .unwrap_or_else(|| panic!("no access log for {block}: {logs:?}"))
    };
    let plain = log("tcp \"web\"");
    assert_eq!(
        plain["upstream"],
        format!("127.0.0.1:{}", proksi.http_upstream.port)
    );
    assert!(plain["client_ip"].as_str().unwrap().starts_with("127.0.0.1"), "{plain}");
    assert!(plain["bytes_in"].as_u64().unwrap() > 40, "{plain}");
    assert!(plain["bytes_out"].as_u64().unwrap() > 100, "{plain}");
    assert!(plain["duration_ms"].as_u64().is_some(), "{plain}");
    assert_eq!(log("tcp \"app\" in host \"a.test\"")["server_name"], "a.test");

    proksi.assert_healthy();
}

const TCP_ON_THE_HTTPS_PORT: &str = r#"
host "game.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  # proksi's own HTTP port, which redirects game.test to HTTPS
  tcp "game" {
    listen = "127.0.0.1:{https_port}"
    upstream "127.0.0.1:{http_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_tcp_on_the_https_port() {
    let mut proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        TCP_ON_THE_HTTPS_PORT,
    )
    .await;
    let get =
        |host: &str| format!("GET /game HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");

    // game.test is TCP: the bytes reach the HTTP port, which redirects them
    let res = proksi
        .tls_request(proksi.https_port, "game.test", &get("game.test"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");

    // Other names on the same port are still HTTPS, over HTTP/1.1 and HTTP/2
    let res = proksi
        .tls_request(proksi.https_port, "app.test", &get("app.test"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/game\""), "{res}");

    // HTTP/2 answers with a SETTINGS frame (type 4)
    let frame = proksi
        .h2_first_frame(proksi.https_port, "app.test")
        .await
        .unwrap();
    assert_eq!(frame[3], 4, "{frame:?}");

    proksi.assert_healthy();
}

const HTTPS_AND_TCP_ON_ONE_HOST: &str = r#"
host "game.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }

  # proksi's own HTTP port, which redirects game.test to HTTPS
  tcp "game" {
    listen = "127.0.0.1:{https_port}"
    alpn   = ["game/1"]
    upstream "127.0.0.1:{http_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_https_and_tcp_on_one_host_chosen_by_alpn() {
    let mut proksi = Harness::start_with(vec![], HTTPS_AND_TCP_ON_ONE_HOST).await;
    let get = "GET /game HTTP/1.1\r\nHost: game.test\r\nConnection: close\r\n\r\n";

    // Asking for the block's protocol: TCP, so the bytes reach the HTTP port (a redirect)
    let res = proksi
        .tls_request_with_alpn(proksi.https_port, "game.test", &["game/1"], get)
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");

    // Otherwise HTTPS, served by the host's location
    for alpn in [&[][..], &["http/1.1"], &["other"]] {
        let res = proksi
            .tls_request_with_alpn(proksi.https_port, "game.test", alpn, get)
            .await
            .unwrap();
        assert!(res.starts_with("HTTP/1.1 200"), "{alpn:?}: {res}");
        assert!(res.contains("\"path\":\"/game\""), "{res}");
    }
    let frame = proksi
        .h2_first_frame(proksi.https_port, "game.test")
        .await
        .unwrap();
    assert_eq!(frame[3], 4, "{frame:?}");

    proksi.assert_healthy();
}

const PASSTHROUGH_ON_THE_HTTPS_PORT: &str = r#"
# Only served on its own address
host "backend.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }
  listen = ["127.0.0.1:{tcp_port_1}"]

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}

# Forwarded untouched to backend.test's address, which terminates TLS itself
host "pass.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  tcp "raw" {
    listen = "127.0.0.1:{https_port}"
    tls    = "passthrough"
    upstream "127.0.0.1:{tcp_port_1}" {}
  }
}

# TLS terminated by proksi, on the same port
host "game.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  tcp "game" {
    listen = "127.0.0.1:{https_port}"
    upstream "127.0.0.1:{http_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_tls_passthrough_on_the_https_port() {
    let mut proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        PASSTHROUGH_ON_THE_HTTPS_PORT,
    )
    .await;
    let get = |host: &str, path: &str| {
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
    };

    // Asking for pass.test: the connection reaches backend.test's address untouched
    let res = proksi
        .tls_request(proksi.https_port, "pass.test", &get("backend.test", "/pass"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/pass\""), "{res}");

    // Other names are terminated by proksi: TCP...
    let res = proksi
        .tls_request(proksi.https_port, "game.test", &get("game.test", "/"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");
    // ...and HTTPS, over HTTP/1.1 and HTTP/2
    let res = proksi
        .tls_request(proksi.https_port, "app.test", &get("app.test", "/app"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/app\""), "{res}");
    let frame = proksi
        .h2_first_frame(proksi.https_port, "app.test")
        .await
        .unwrap();
    assert_eq!(frame[3], 4, "{frame:?}");
    // backend.test isn't served on the HTTPS port itself
    let res = proksi
        .tls_request(proksi.https_port, "app.test", &get("backend.test", "/"))
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");

    // The passthrough connection's access log has the name it asked for
    let logs = proksi.tcp_access_logs(2).await;
    let passthrough = logs
        .iter()
        .find(|log| log["block"] == "tcp \"raw\" in host \"pass.test\"")
        .unwrap_or_else(|| panic!("{logs:?}"));
    assert_eq!(passthrough["server_name"], "pass.test");

    proksi.assert_healthy();
}

const PLAIN_HTTP_HOSTS: &str = r#"
host "plain.test" {
  tls = false

  location "/" {
    headers {
      response {
        add = { "x-host" = "plain" }
      }
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}

host "*" {
  tls = false

  location "/" {
    headers {
      response {
        add = { "x-host" = "catch-all" }
      }
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_hosts_without_tls_are_served_on_the_http_port() {
    let mut proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        PLAIN_HTTP_HOSTS,
    )
    .await;

    let res = proksi.get_http_port("plain.test", "/hello").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.to_lowercase().contains("x-host: plain"), "{res}");
    assert!(res.contains("\"path\":\"/hello\""), "{res}");

    // Hosts without TLS get no certificates, so ACME paths go to the upstream too
    let res = proksi
        .get_http_port("plain.test", "/.well-known/acme-challenge/token")
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.to_lowercase().contains("x-host: plain"), "{res}");

    // Any other name is served by `host "*"`
    let res = proksi.get_http_port("other.test", "/").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.to_lowercase().contains("x-host: catch-all"), "{res}");

    // Hosts with TLS are still redirected, and ACME challenges answered as before
    let res = proksi.get_http_port("app.test", "/path").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");
    let res = proksi.get_http_port("other.test", "/ping").await.unwrap();
    assert!(
        res.starts_with("HTTP/1.1 200") && res.ends_with("pong"),
        "{res}"
    );

    proksi.assert_healthy();
}

const CATCH_ALL_WITHOUT_TLS: &str = r#"
host "*" {
  tls = false

  location "/" {
    headers {
      response {
        add = { "x-host" = "catch-all" }
      }
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_runs_without_https() {
    let mut proksi = Harness::start_without_https(CATCH_ALL_WITHOUT_TLS).await;

    let res = proksi
        .get_http_port("anything.test", "/hello")
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.to_lowercase().contains("x-host: catch-all"), "{res}");

    // Nothing listens on the HTTPS port
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", proksi.https_port))
            .await
            .is_err()
    );

    proksi.assert_healthy();
}

const CATCH_ALL_ON_ANOTHER_PORT: &str = r#"
host "*" {
  tls = false
  listen = ["127.0.0.1:{extra_port}"]

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_hosts_are_only_served_on_their_addresses() {
    let mut proksi = Harness::start_without_https(CATCH_ALL_ON_ANOTHER_PORT).await;

    let res = proksi
        .get_http_on(proksi.extra_port, "other.test", "/")
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    // `host "*"` doesn't listen on the default HTTP port
    let res = proksi.get_http_port("other.test", "/").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");

    proksi.assert_healthy();
}

const ADMIN: &str = r#"
admin {
  listen = "127.0.0.1:{tcp_port_2}"

  token "e2e" {
    secret = "e2e-token"
    access = "write"
  }
}

host "limited.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/" {
    rate_limit {
      requests = 1
      per      = "1m"
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}

tcp "web" {
  listen = "127.0.0.1:{tcp_port_0}"
  upstream "127.0.0.1:{http_upstream_port}" {}
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_admin_metrics() {
    let mut proksi = Harness::start_with(vec![Route::new("app.test", Upstream::Http)], ADMIN).await;
    let admin_port = proksi.tcp_ports[2];
    let metrics = |token: Option<&str>| {
        let authorization = token
            .map(|token| format!("Authorization: Bearer {token}\r\n"))
            .unwrap_or_default();
        format!("GET /metrics HTTP/1.1\r\nHost: admin\r\n{authorization}Connection: close\r\n\r\n")
    };

    proksi.resolve("app.test");
    assert_eq!(proksi.get("app.test", "/hello").await.unwrap().status, 200);
    assert_eq!(proksi.get("app.test", "/hello").await.unwrap().status, 200);
    // A host proksi doesn't serve (over a connection for one it does)
    let res = proksi
        .tls_request(
            proksi.https_port,
            "app.test",
            "GET / HTTP/1.1\r\nHost: unknown.test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");

    // Answered by `rate_limit` the second time
    proksi.resolve("limited.test");
    assert_eq!(proksi.get("limited.test", "/").await.unwrap().status, 200);
    assert_eq!(proksi.get("limited.test", "/").await.unwrap().status, 429);
    // Through a tcp block
    let res = proksi
        .raw_http_on(
            proksi.tcp_ports[0],
            b"GET /tcp HTTP/1.1\r\nHost: tcp.test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    let res = proksi
        .raw_http_on(admin_port, metrics(None).as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");

    // Counted once proksi is done with them, just after the client: the tcp connection when
    // proksi has closed its side too, and the `429` once it has been written
    let recorded = [
        "proksi_tcp_connections_active{block=\"tcp \\\"web\\\"\"} 0",
        "proksi_blocked_requests_total{block=\"rate_limit\",host=\"limited.test\",location=\"/\"} 1",
        "proksi_requests_total{host=\"limited.test\",location=\"/\",status=\"4xx\"} 1",
        // By status code too
        "proksi_responses_total{code=\"429\",host=\"limited.test\",location=\"/\"} 1",
    ];
    let mut res = String::new();
    for _ in 0..20 {
        res = proksi
            .raw_http_on(admin_port, metrics(Some("e2e-token")).as_bytes())
            .await
            .unwrap();
        if recorded.iter().all(|line| res.lines().any(|l| l.trim_end() == *line)) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    for line in recorded {
        assert!(res.lines().any(|l| l.trim_end() == line), "{line}\n{res}");
    }
    let line = |prefix: &str, suffix: &str| {
        res.lines()
            .find(|line| line.starts_with(prefix) && line.contains(suffix))
            .map(|line| line.rsplit(' ').next().unwrap_or_default().to_string())
    };
    // By host and location, and status class
    assert_eq!(
        line("proksi_requests_total{host=\"app.test\"", "status=\"2xx\"").as_deref(),
        Some("2"),
        "{res}"
    );
    assert_eq!(
        line("proksi_requests_total{host=\"_unknown\"", "status=\"4xx\"").as_deref(),
        Some("1"),
        "{res}"
    );
    // Time to the upstream's response headers, by upstream
    assert_eq!(
        line(
            "proksi_upstream_duration_seconds_count{host=\"app.test\"",
            &format!("upstream=\"127.0.0.1:{}\"", proksi.http_upstream.port)
        )
        .as_deref(),
        Some("2"),
        "{res}"
    );
    assert_eq!(
        line(
            "proksi_upstream_healthy{host=\"app.test\"",
            &format!("upstream=\"127.0.0.1:{}\"", proksi.http_upstream.port)
        )
        .as_deref(),
        Some("1"),
        "{res}"
    );
    // Building blocks
    assert_eq!(
        line(
            "proksi_blocked_requests_total{block=\"rate_limit\",host=\"limited.test\"",
            ""
        )
        .as_deref(),
        Some("1"),
        "{res}"
    );
    assert_eq!(
        line(
            "proksi_requests_total{host=\"limited.test\"",
            "status=\"4xx\""
        )
        .as_deref(),
        Some("1"),
        "{res}"
    );
    // TCP: the connection closed once the upstream answered
    assert_eq!(
        line("proksi_tcp_connections_total{block=\"tcp \\\"web\\\"\"}", "").as_deref(),
        Some("1"),
        "{res}"
    );
    assert_eq!(
        line("proksi_tcp_connections_active{block=\"tcp \\\"web\\\"\"}", "").as_deref(),
        Some("0"),
        "{res}"
    );
    let bytes = |direction: &str| {
        line(
            "proksi_tcp_bytes_total{block=\"tcp \\\"web\\\"\"",
            &format!("direction=\"{direction}\""),
        )
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default()
    };
    assert!(bytes("in") > 40 && bytes("out") > 100, "{res}");
    // Hosts by source
    assert!(line("proksi_routes{source=\"file\"}", "").is_some(), "{res}");

    // Certificate expiry, refreshed 5 seconds after starting
    let mut expiry = None;
    for _ in 0..30 {
        let res = proksi
            .raw_http_on(admin_port, metrics(Some("e2e-token")).as_bytes())
            .await
            .unwrap();
        expiry = res
            .lines()
            .find(|line| {
                line.starts_with(
                    "proksi_certificate_expiry_timestamp_seconds{domain=\"limited.test\"}",
                )
            })
            .map(ToString::to_string);
        if expiry.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(expiry.is_some(), "no certificate expiry for limited.test");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_admin_live_stats() {
    let mut proksi = Harness::start_with(vec![Route::new("app.test", Upstream::Http)], ADMIN).await;
    let admin_port = proksi.tcp_ports[2];
    let get = |path: &str| {
        format!(
            "GET {path} HTTP/1.1\r\nHost: admin\r\nAuthorization: Bearer e2e-token\r\nConnection: close\r\n\r\n"
        )
    };
    let body = |res: &str| res.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();

    // Nothing is sampled until the stats are read: the first read starts it
    let res = proksi
        .raw_http_on(admin_port, get("/api/v1/stats").as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    proksi.resolve("app.test");
    for _ in 0..5 {
        assert_eq!(proksi.get("app.test", "/live").await.unwrap().status, 200);
    }
    // Sampled every second
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let res = proksi
        .raw_http_on(admin_port, get("/api/v1/stats?window=60").as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let stats: serde_json::Value = serde_json::from_str(&body(&res)).unwrap();
    let host = stats["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["host"] == "app.test")
        .unwrap_or_else(|| panic!("{stats}"));
    let location = &host["locations"][0];
    assert_eq!(location["responses"]["2xx"], 5, "{stats}");
    assert!(location["requests_per_second"].as_f64().unwrap() > 0.0, "{stats}");
    assert!(location["duration_ms"]["p50"].as_f64().is_some(), "{stats}");
    assert_eq!(location["upstreams"][0]["healthy"], true, "{stats}");

    let res = proksi
        .raw_http_on(admin_port, get("/api/v1/stats?window=0").as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 400"), "{res}");

    // The stream sends the stats every second
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", admin_port))
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(
        &mut stream,
        get("/api/v1/stats/stream?window=10").as_bytes(),
    )
    .await
    .unwrap();
    let mut received = String::new();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while received.matches("data: ").count() < 2 && tokio::time::Instant::now() < deadline {
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio::io::AsyncReadExt::read(&mut stream, &mut buf),
        )
        .await
        .unwrap()
        .unwrap();
        received.push_str(&String::from_utf8_lossy(&buf[..read]));
    }
    assert!(received.starts_with("HTTP/1.1 200"), "{received}");
    assert!(received.to_lowercase().contains("content-type: text/event-stream"), "{received}");
    let event = received
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap();
    let event: serde_json::Value = serde_json::from_str(event).unwrap();
    assert!(event["hosts"].is_array(), "{event}");
    assert!(event["total"]["requests_per_second"].is_number(), "{event}");
    assert!(event["process"]["memory_bytes"].as_u64().unwrap() > 0, "{event}");

    // The requests stream sends each request served while it's open
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", admin_port))
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut stream, get("/api/v1/requests/stream?host=APP.test").as_bytes())
        .await
        .unwrap();
    let mut received = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !received.contains("\"path\":\"/watched\"") && tokio::time::Instant::now() < deadline {
        proksi.get_http_port("app.test", "/watched").await.unwrap();
        if let Ok(Ok(read)) = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::io::AsyncReadExt::read(&mut stream, &mut buf),
        )
        .await
        {
            received.push_str(&String::from_utf8_lossy(&buf[..read]));
        }
    }
    let event = received
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find(|event| event.contains("/watched"))
        .unwrap_or_else(|| panic!("{received}"));
    let event: serde_json::Value = serde_json::from_str(event).unwrap();
    assert_eq!(event["host"], "app.test", "{event}");
    assert_eq!(event["method"], "GET", "{event}");
    assert!(event["status"].is_number(), "{event}");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_admin_status_hosts_and_certificates() {
    let mut proksi = Harness::start_with(vec![Route::new("app.test", Upstream::Http)], ADMIN).await;
    let admin_port = proksi.tcp_ports[2];
    let get = |path: &str| {
        format!(
            "GET {path} HTTP/1.1\r\nHost: admin\r\nAuthorization: Bearer e2e-token\r\nConnection: close\r\n\r\n"
        )
    };
    let json = |res: String| -> serde_json::Value {
        assert!(res.starts_with("HTTP/1.1 200"), "{res}");
        serde_json::from_str(res.split("\r\n\r\n").nth(1).unwrap_or_default()).unwrap()
    };

    let status = json(
        proksi
            .raw_http_on(admin_port, get("/api/v1/status").as_bytes())
            .await
            .unwrap(),
    );
    assert_eq!(status["store"]["type"], "memory", "{status}");
    assert_eq!(
        status["listeners"]["tcp"][0],
        format!("127.0.0.1:{}", proksi.tcp_ports[0]),
        "{status}"
    );
    assert!(status["routes"]["by_source"]["file"].as_u64().unwrap() >= 2, "{status}");

    let hosts = json(
        proksi
            .raw_http_on(admin_port, get("/api/v1/hosts").as_bytes())
            .await
            .unwrap(),
    );
    let limited = hosts
        .as_array()
        .unwrap()
        .iter()
        .find(|host| host["host"] == "limited.test")
        .unwrap_or_else(|| panic!("{hosts}"));
    assert_eq!(limited["source"], "file");
    assert_eq!(limited["locations"][0]["location"], "/");
    assert_eq!(limited["locations"][0]["blocks"], serde_json::json!(["rate_limit"]));
    assert_eq!(limited["locations"][0]["upstreams"][0]["healthy"], true);

    let certificates = json(
        proksi
            .raw_http_on(admin_port, get("/api/v1/certificates").as_bytes())
            .await
            .unwrap(),
    );
    let certificate = certificates
        .as_array()
        .unwrap()
        .iter()
        .find(|certificate| certificate["domain"] == "limited.test")
        .unwrap_or_else(|| panic!("{certificates}"));
    assert!(certificate["days_left"].as_i64().unwrap() > 0, "{certificate}");
    assert!(certificate["expires_at"].as_i64().is_some(), "{certificate}");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_admin_routes() {
    let mut proksi = Harness::start_with(vec![], ADMIN).await;
    let admin_port = proksi.tcp_ports[2];
    let request = |method: &str, path: &str, content_type: &str, body: &str| {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: admin\r\nAuthorization: Bearer e2e-token\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    let served = |status: &'static str| {
        let proksi = &proksi;
        async move {
            for _ in 0..50 {
                let res = proksi.get_http_port("api.test", "/hello").await.unwrap();
                if res.starts_with(status) {
                    return res;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            panic!("api.test never answered {status}");
        }
    };

    // JSON, in the stored format
    let json = format!(
        r#"{{ "name": "api.test", "tls": {{ "enabled": false }}, "locations": [{{ "path": "/", "upstreams": [{{ "ip": "127.0.0.1", "port": {} }}] }}] }}"#,
        proksi.http_upstream.port
    );
    let res = proksi
        .raw_http_on(
            admin_port,
            request("PUT", "/api/v1/routes/api.test", "application/json", &json).as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains(r#""host":"api.test""#), "{res}");
    let res = served("HTTP/1.1 200").await;
    assert!(res.contains("\"path\":\"/hello\""), "{res}");

    // One host, as stored
    let res = proksi
        .raw_http_on(
            admin_port,
            request("GET", "/api/v1/routes/api.test", "application/json", "").as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains(r#""revision":"#), "{res}");

    // The API takes JSON only: HCL is for configuration files
    let hcl = "tls = false\nlocation \"/\" {}\n";
    let res = proksi
        .raw_http_on(
            admin_port,
            request("PUT", "/api/v1/routes/api.test", "text/hcl", hcl).as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 400"), "{res}");

    // Invalid hosts are refused with the reason
    let res = proksi
        .raw_http_on(
            admin_port,
            request("PUT", "/api/v1/routes/api.test", "application/json", "{").as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 400"), "{res}");
    assert!(res.contains("invalid JSON"), "{res}");

    let res = proksi
        .raw_http_on(
            admin_port,
            request("GET", "/api/v1/routes", "application/json", "").as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.contains(r#""name":"api.test""#), "{res}");

    let res = proksi
        .raw_http_on(
            admin_port,
            request("DELETE", "/api/v1/routes/api.test", "application/json", "").as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    // Unknown again: redirected to HTTPS
    served("HTTP/1.1 308").await;

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_forwarded_headers() {
    let get = |forwarded: &str| {
        format!(
            "GET /forwarded HTTP/1.1\r\nHost: app.test\r\n{forwarded}Connection: close\r\n\r\n"
        )
    };
    let forged = "X-Forwarded-For: 1.2.3.4\r\nX-Forwarded-Proto: http\r\nX-Forwarded-Host: forged.test\r\n";
    let headers = |res: &str| -> serde_json::Value {
        let body: serde_json::Value =
            serde_json::from_str(res.split("\r\n\r\n").nth(1).unwrap_or_default()).unwrap();
        body["headers"].clone()
    };

    // A client's own headers are replaced
    let mut proksi = Harness::start(vec![Route::new("app.test", Upstream::Http)]).await;
    let res = proksi
        .tls_request(proksi.https_port, "app.test", &get(forged))
        .await
        .unwrap();
    let sent = headers(&res);
    assert_eq!(sent["x-forwarded-for"], "127.0.0.1", "{sent}");
    assert_eq!(sent["x-forwarded-proto"], "https", "{sent}");
    assert_eq!(sent["x-forwarded-host"], "app.test", "{sent}");
    assert_eq!(sent["x-real-ip"], "127.0.0.1", "{sent}");
    proksi.assert_healthy();

    // A trusted proxy's are kept, with its address appended
    let mut proksi = Harness::start_with_server(
        vec![Route::new("app.test", Upstream::Http)],
        "trusted_proxies = [\"127.0.0.1\"]",
        "",
    )
    .await;
    let res = proksi
        .tls_request(proksi.https_port, "app.test", &get(forged))
        .await
        .unwrap();
    let sent = headers(&res);
    assert_eq!(sent["x-forwarded-for"], "1.2.3.4, 127.0.0.1", "{sent}");
    assert_eq!(sent["x-forwarded-proto"], "http", "{sent}");
    assert_eq!(sent["x-forwarded-host"], "forged.test", "{sent}");
    assert_eq!(sent["x-real-ip"], "1.2.3.4", "{sent}");
    proksi.assert_healthy();
}

const ADMIN_WITHOUT_METRICS: &str = r#"
admin {
  listen  = "127.0.0.1:{tcp_port_2}"
  metrics = false

  token "e2e" {
    secret = "e2e-token"
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_admin_without_metrics() {
    let mut proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        ADMIN_WITHOUT_METRICS,
    )
    .await;
    let get = |path: &str| {
        format!(
            "GET {path} HTTP/1.1\r\nHost: admin\r\nAuthorization: Bearer e2e-token\r\nConnection: close\r\n\r\n"
        )
    };
    proksi.resolve("app.test");
    assert_eq!(proksi.get("app.test", "/").await.unwrap().status, 200);

    for path in ["/metrics", "/api/v1/stats", "/api/v1/stats/stream"] {
        let res = proksi
            .raw_http_on(proksi.tcp_ports[2], get(path).as_bytes())
            .await
            .unwrap();
        assert!(res.starts_with("HTTP/1.1 404"), "{path}: {res}");
        assert!(res.contains("metrics are disabled (admin.metrics = false)"), "{res}");
    }
    // The rest of the API still works
    let res = proksi
        .raw_http_on(proksi.tcp_ports[2], get("/api/v1/status").as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    proksi.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_certificate_files_are_served_with_their_chain() {
    let mut proksi = Harness::start_with(
        vec![],
        r#"
host "chain.test" {
  tls {
    certificate {
      pem = "{chain_pem}"
      key = "{chain_key}"
    }
  }
  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;
    proksi.resolve("chain.test");
    proksi.wait_for("chain.test", "/", 200).await;

    // The leaf and both intermediates, verified against the root alone
    assert_eq!(
        proksi.verified_handshake(proksi.https_port, "chain.test").await,
        Ok(3)
    );
    proksi.assert_healthy();
}

const TCP_BLOCK: &str = r#"
tcp "web" {
  listen = "127.0.0.1:{tcp_port_0}"
  upstream "127.0.0.1:{http_upstream_port}" {}
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_stops_right_away_without_connections() {
    let mut proksi = Harness::start_with_server(
        vec![Route::new("app.test", Upstream::Http)],
        "shutdown_timeout_secs = 30",
        TCP_BLOCK,
    )
    .await;
    // A connection kept alive by the client, idle: closed by proksi when it stops
    let client = proksi.client();
    let res = client
        .get(format!("https://app.test:{}/", proksi.https_port))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    proksi.terminate();

    assert!(proksi.exits_within(Duration::from_secs(3)).await);
    drop(client);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stopping_waits_for_open_connections() {
    let mut proksi =
        Harness::start_with_server(vec![], "shutdown_timeout_secs = 30", TCP_BLOCK).await;
    let connection = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    proksi.terminate();
    assert!(!proksi.exits_within(Duration::from_secs(2)).await);

    // Stops once the connection closes
    drop(connection);
    assert!(proksi.exits_within(Duration::from_secs(3)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_stopping_closes_connections_after_the_timeout() {
    let mut proksi =
        Harness::start_with_server(vec![], "shutdown_timeout_secs = 2", TCP_BLOCK).await;
    let _connection = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    proksi.terminate();

    assert!(!proksi.exits_within(Duration::from_secs(1)).await);
    assert!(proksi.exits_within(Duration::from_secs(4)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_certificate_files_are_reloaded_when_they_change() {
    let dir = tempfile::tempdir().unwrap();
    let (pem, key) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    let write = |name: &str| {
        let (certificate, private) = harness::signed_certificate(name, None, false);
        std::fs::write(&pem, certificate.to_pem().unwrap()).unwrap();
        std::fs::write(&key, private.private_key_to_pem_pkcs8().unwrap()).unwrap();
        certificate.to_pem().unwrap()
    };
    let first = write("files.test");

    let config = format!(
        r#"
host "files.test" {{
  tls {{
    certificate {{
      pem = "{}"
      key = "{}"
    }}
  }}
  location "/" {{
    upstream "127.0.0.1:{{http_upstream_port}}" {{}}
  }}
}}
"#,
        pem.display(),
        key.display()
    );
    let mut proksi = Harness::start_with(vec![], &config).await;
    proksi.resolve("files.test");
    proksi.wait_for("files.test", "/", 200).await;
    let served = proksi.served_certificate(proksi.https_port, "files.test").await;
    assert_eq!(served.to_pem().unwrap(), first);

    // Renewed by another tool, without touching the configuration
    let renewed = write("files.test");
    let mut reloaded = false;
    for _ in 0..30 {
        let served = proksi.served_certificate(proksi.https_port, "files.test").await;
        if served.to_pem().unwrap() == renewed {
            reloaded = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(reloaded, "the renewed certificate wasn't served");
    proksi.assert_healthy();
}

const PLAIN_HOST: &str = r#"
host "plain.test" {
  tls = false
  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

/// Whether proksi closes `stream` within `timeout`
async fn closed_within(stream: &mut tokio::net::TcpStream, timeout: std::time::Duration) -> bool {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 4096];
    let read_until_closed = async {
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    };
    tokio::time::timeout(timeout, read_until_closed).await.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn test_idle_client_connections_are_closed() {
    use tokio::io::AsyncWriteExt;
    let proksi = Harness::start_with_server(
        vec![Route::new("app.test", Upstream::Http)],
        "timeouts {\n idle_secs = 1\n }",
        PLAIN_HOST,
    )
    .await;

    // A connection that never sends a request
    let mut silent = tokio::net::TcpStream::connect(("127.0.0.1", proksi.http_port))
        .await
        .unwrap();
    assert!(closed_within(&mut silent, std::time::Duration::from_secs(3)).await);

    // A connection kept alive after its request
    let mut kept = tokio::net::TcpStream::connect(("127.0.0.1", proksi.http_port))
        .await
        .unwrap();
    kept.write_all(b"GET / HTTP/1.1\r\nHost: plain.test\r\n\r\n")
        .await
        .unwrap();
    assert!(closed_within(&mut kept, std::time::Duration::from_secs(3)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_slow_request_bodies_are_closed() {
    use tokio::io::AsyncWriteExt;
    let proksi = Harness::start_with_server(
        vec![Route::new("app.test", Upstream::Http)],
        "timeouts {\n read_secs = 1\n }",
        PLAIN_HOST,
    )
    .await;

    // A body that stops arriving
    let mut slow = tokio::net::TcpStream::connect(("127.0.0.1", proksi.http_port))
        .await
        .unwrap();
    slow.write_all(b"POST / HTTP/1.1\r\nHost: plain.test\r\nContent-Length: 100\r\n\r\nab")
        .await
        .unwrap();
    assert!(closed_within(&mut slow, std::time::Duration::from_secs(3)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_locations_set_their_own_timeouts() {
    use tokio::io::AsyncWriteExt;
    let proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        r#"
host "plain.test" {
  tls = false
  timeouts {
    read_secs = 1
  }
  location "/upload" {
    timeouts {
      read_secs = 30
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;

    let slow_body = |path: &'static str| {
        let port = proksi.http_port;
        async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!("POST {path} HTTP/1.1\r\nHost: plain.test\r\nContent-Length: 100\r\n\r\nab")
                        .as_bytes(),
                )
                .await
                .unwrap();
            stream
        }
    };

    // The host's timeout, shorter than the server's
    let mut root = slow_body("/").await;
    assert!(closed_within(&mut root, std::time::Duration::from_secs(3)).await);

    // The location's, longer than the host's
    let mut upload = slow_body("/upload").await;
    assert!(!closed_within(&mut upload, std::time::Duration::from_secs(3)).await);
}

const TLS_HOST: &str = r#"
host "tls.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }
  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

/// The cipher negotiated with proksi on `port`, offering only the TLS 1.2 `ciphers`, or
/// OpenSSL's defaults without any; or why the handshake failed
async fn negotiated_cipher(port: u16, ciphers: Option<&'static str>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode, SslVersion};
        let mut connector = SslConnector::builder(SslMethod::tls()).unwrap();
        connector.set_verify(SslVerifyMode::NONE);
        if let Some(ciphers) = ciphers {
            connector.set_max_proto_version(Some(SslVersion::TLS1_2)).unwrap();
            connector.set_cipher_list(ciphers).unwrap();
        }
        let stream =
            std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|err| err.to_string())?;
        let stream = connector
            .build()
            .connect("tls.test", stream)
            .map_err(|err| err.to_string())?;
        let cipher = stream.ssl().current_cipher();
        Ok(cipher.map(|cipher| cipher.name().to_string()).unwrap_or_default())
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn test_dhe_ciphers_are_refused_by_default() {
    let proksi = Harness::start_with(vec![], TLS_HOST).await;

    let refused = negotiated_cipher(proksi.https_port, Some("DHE-RSA-AES128-GCM-SHA256")).await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        negotiated_cipher(proksi.https_port, Some("ECDHE-RSA-AES128-GCM-SHA256")).await,
        Ok("ECDHE-RSA-AES128-GCM-SHA256".to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tls_ciphers_can_be_chosen() {
    let proksi = Harness::start_with_server(
        vec![],
        "tls {\n ciphers = [\"ECDHE-RSA-AES256-GCM-SHA384\"]\n }",
        TLS_HOST,
    )
    .await;

    let refused = negotiated_cipher(proksi.https_port, Some("ECDHE-RSA-AES128-GCM-SHA256")).await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(
        negotiated_cipher(proksi.https_port, Some("ECDHE-RSA-AES256-GCM-SHA384")).await,
        Ok("ECDHE-RSA-AES256-GCM-SHA384".to_string())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tls_1_2_can_be_refused() {
    let proksi =
        Harness::start_with_server_ready_over_http("tls {\n min_version = \"1.3\"\n }", TLS_HOST)
            .await;

    let refused = negotiated_cipher(proksi.https_port, Some("ECDHE-RSA-AES128-GCM-SHA256")).await;
    assert!(refused.is_err(), "{refused:?}");
    assert!(negotiated_cipher(proksi.https_port, None).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_request_bodies_must_arrive_in_time() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let proksi = Harness::start_with_server(
        vec![],
        "timeouts {\n body_secs = 1\n }",
        r#"
host "plain.test" {
  tls = false
  location "/upload" {
    timeouts {
      body_secs = 30
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;

    // A byte every 300 ms: each read is quick, the whole body isn't. Returns how long until
    // proksi gave up on it, if it did within 3 seconds.
    let trickle = |path: &'static str| {
        let port = proksi.http_port;
        async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!("POST {path} HTTP/1.1\r\nHost: plain.test\r\nContent-Length: 100\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let started = std::time::Instant::now();
            let mut buf = [0u8; 1024];
            while started.elapsed() < std::time::Duration::from_secs(3) {
                if stream.write_all(b"a").await.is_err() {
                    return Some((started.elapsed(), String::new()));
                }
                match tokio::time::timeout(
                    std::time::Duration::from_millis(300),
                    stream.read(&mut buf),
                )
                .await
                {
                    Ok(Ok(0) | Err(_)) => return Some((started.elapsed(), String::new())),
                    Ok(Ok(read)) => {
                        return Some((
                            started.elapsed(),
                            String::from_utf8_lossy(&buf[..read]).to_string(),
                        ));
                    }
                    Err(_) => {}
                }
            }
            None
        }
    };

    let (elapsed, res) = trickle("/").await.expect("the body was still accepted after 3 s");
    assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    assert!(res.is_empty() || res.starts_with("HTTP/1.1 408"), "{res}");

    // The location's limit, longer
    assert!(trickle("/upload").await.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_metrics_on_a_listener_of_their_own() {
    let proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        r#"
admin {
  listen         = "127.0.0.1:{tcp_port_2}"
  metrics_listen = "127.0.0.1:{tcp_port_1}"

  token "ops" {
    secret = "admin-token"
  }
}
"#,
    )
    .await;
    let get = |port: u16, path: &str| {
        let request = format!("GET {path} HTTP/1.1\r\nHost: admin\r\nConnection: close\r\n\r\n");
        let proksi = &proksi;
        async move { proksi.raw_http_on(port, request.as_bytes()).await.unwrap() }
    };

    let metrics = get(proksi.tcp_ports[1], "/metrics").await;
    assert!(metrics.starts_with("HTTP/1.1 200"), "{metrics}");
    assert!(metrics.contains("proksi_requests_total"), "{metrics}");
    // Nothing else, not even with the admin API's token
    let routes = get(proksi.tcp_ports[1], "/api/v1/routes").await;
    assert!(routes.starts_with("HTTP/1.1 404"), "{routes}");
    // The admin API still needs its token
    let admin = get(proksi.tcp_ports[2], "/metrics").await;
    assert!(admin.starts_with("HTTP/1.1 401"), "{admin}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_failed_tls_handshakes_and_tcp_upstream_errors_are_counted() {
    use tokio::io::AsyncReadExt;
    // Nothing listens on port 1 (a port released by another test could be taken again by the
    // tests running alongside)
    let refused_port = 1;
    let proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        &format!(
            r#"
admin {{
  listen = "127.0.0.1:{{tcp_port_2}}"
}}

tcp "gone" {{
  listen = "127.0.0.1:{{tcp_port_0}}"
  upstream "127.0.0.1:{refused_port}" {{}}
}}
"#
        ),
    )
    .await;

    // A client that doesn't trust proksi's (self-signed) certificate gives up mid-handshake
    let https_port = proksi.https_port;
    let handshake = tokio::task::spawn_blocking(move || {
        let connector = openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls())
            .unwrap()
            .build();
        let stream = std::net::TcpStream::connect(("127.0.0.1", https_port)).unwrap();
        connector.connect("app.test", stream).map(|_| ())
    })
    .await
    .unwrap();
    assert!(handshake.is_err());
    // A tcp connection whose upstream refuses it
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
        .await
        .unwrap();
    let mut buf = [0u8; 16];
    let closed = tokio::time::timeout(Duration::from_secs(5), client.read(&mut buf)).await;
    assert!(closed.is_ok(), "proksi didn't close the connection\n{}", proksi.logs());

    let expected = [
        "proksi_tls_handshake_failures_total 1",
        "proksi_tcp_upstream_errors_total{block=\"tcp \\\"gone\\\"\"} 1",
    ];
    let mut metrics = String::new();
    for _ in 0..30 {
        metrics = proksi
            .raw_http_on(
                proksi.tcp_ports[2],
                b"GET /metrics HTTP/1.1\r\nHost: admin\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        if expected.iter().all(|line| metrics.lines().any(|l| l.trim_end() == *line)) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    for line in expected {
        assert!(metrics.lines().any(|l| l.trim_end() == line), "{line}\n{metrics}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_clients_giving_up_on_tls_handshakes_are_not_logged_as_errors() {
    use tokio::io::AsyncWriteExt;
    let proksi = Harness::start_with(vec![Route::new("app.test", Upstream::Http)], "").await;

    // The start of a ClientHello, then the client goes away (as scanners and health checks do)
    for _ in 0..3 {
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", proksi.https_port))
            .await
            .unwrap();
        client.write_all(&[0x16, 0x03, 0x01, 0x00, 0x40]).await.unwrap();
    }
    // A request after them: their failures were logged by then, if they were
    assert_eq!(proksi.get("app.test", "/").await.unwrap().status, 200);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let logs = proksi.logs();
    assert!(!logs.contains("Downstream handshake error"), "{logs}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_acme_challenge_locations_are_proxied_on_the_http_port() {
    // As cert-manager's HTTP-01 solver adds one for a host with TLS
    let proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        r#"
host "solver.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }
  location "/.well-known/acme-challenge/token-123" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;

    let res = proksi
        .get_http_port("solver.test", "/.well-known/acme-challenge/token-123")
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains("\"path\":\"/.well-known/acme-challenge/token-123\""), "{res}");

    // Other challenges are proksi's own (none here), other paths redirected to HTTPS
    let res = proksi
        .get_http_port("solver.test", "/.well-known/acme-challenge/other")
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");
    let res = proksi.get_http_port("solver.test", "/page").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 308"), "{res}");
}

/// Upgrades hand the listening sockets over to a new process, which needs Linux
#[cfg(target_os = "linux")]
mod upgrades {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{Harness, Route, Upstream};

    const CONFIG: &str = r#"
auto_reload {
  enabled = true
  interval_secs = 1
}

tcp "web" {
  listen = "127.0.0.1:{tcp_port_0}"
  upstream "127.0.0.1:{http_upstream_port}" {}
}
"#;

    /// Sends requests to `app.test` until stopped, counting the ones that failed
    fn requests_in_a_loop(proksi: &Harness) -> (Arc<AtomicBool>, Arc<AtomicUsize>, tokio::task::JoinHandle<usize>) {
        let (stop, failed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
        let client = proksi.client();
        let url = format!("https://app.test:{}/", proksi.https_port);
        let (stopping, failures) = (stop.clone(), failed.clone());
        let task = tokio::spawn(async move {
            let mut sent = 0;
            while !stopping.load(Ordering::Relaxed) {
                // A new connection each time, as new clients would
                let res = client.get(&url).header("connection", "close").send().await;
                if !res.is_ok_and(|res| res.status() == 200) {
                    failures.fetch_add(1, Ordering::Relaxed);
                }
                sent += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            sent
        });
        (stop, failed, task)
    }

    /// Waits until a new proksi process has started (counted by its startup log line)
    async fn wait_for_started(proksi: &Harness, processes: usize) {
        for _ in 0..250 {
            if proksi.logs().matches("listening for HTTPS on").count() >= processes {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("no new process started:\n{}", proksi.logs());
    }

    /// Waits until the previous worker has stopped, its connections closed
    async fn wait_for_previous_to_stop(proksi: &Harness) {
        // Pingora keeps accepting for 5 seconds after handing its sockets over
        for _ in 0..100 {
            if proksi.logs().contains("stopped (exit code 0)") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the previous worker didn't stop:\n{}", proksi.logs());
    }

    async fn request_on(connection: &mut tokio::net::TcpStream, path: &str) -> String {
        connection
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: tcp.test\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut buf = vec![0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(5), connection.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8_lossy(&buf[..n]).to_string()
    }

    /// Upgrades with `upgrade`, checking that no new connection fails meanwhile, and that a
    /// connection open before is still served, by the previous process
    async fn assert_upgrade_keeps_connections(upgrade: impl FnOnce(&mut Harness)) {
        let mut proksi = Harness::start_with_server(
            vec![Route::new("app.test", Upstream::Http)],
            "shutdown_timeout_secs = 30",
            CONFIG,
        )
        .await;
        let mut open = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
            .await
            .unwrap();
        assert!(request_on(&mut open, "/before").await.starts_with("HTTP/1.1 200"));
        let (stop, failed, requests) = requests_in_a_loop(&proksi);

        upgrade(&mut proksi);
        wait_for_started(&proksi, 2).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        stop.store(true, Ordering::Relaxed);
        let sent = requests.await.unwrap();

        assert_eq!(failed.load(Ordering::Relaxed), 0, "of {sent}:\n{}", proksi.logs());
        // The connection open before the upgrade is still served
        assert!(request_on(&mut open, "/after").await.starts_with("HTTP/1.1 200"));
        // The previous process stops once it's closed
        drop(open);
        wait_for_previous_to_stop(&proksi).await;
        // And the new one serves new connections
        assert_eq!(proksi.get("app.test", "/").await.unwrap().status, 200);
        assert!(proksi.is_alive());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_settings_changes_keep_connections() {
        assert_upgrade_keeps_connections(|proksi| {
            // Anything but hosts restarts proksi
            proksi.rewrite_extra_config(&format!("{CONFIG}\ndocker {{\n  interval_secs = 99\n}}\n"));
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_sighup_upgrades_without_dropping_connections() {
        assert_upgrade_keeps_connections(|proksi| {
            proksi.signal(nix::sys::signal::Signal::SIGHUP);
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_changes_during_an_upgrade_are_applied_after_it() {
        let proksi = Harness::start_with(vec![Route::new("app.test", Upstream::Http)], CONFIG)
            .await;
        let (stop, failed, requests) = requests_in_a_loop(&proksi);

        proksi.signal(nix::sys::signal::Signal::SIGHUP);
        // While the first upgrade is still handing over
        tokio::time::sleep(Duration::from_millis(100)).await;
        proksi.signal(nix::sys::signal::Signal::SIGHUP);
        wait_for_started(&proksi, 3).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        stop.store(true, Ordering::Relaxed);
        let sent = requests.await.unwrap();

        assert_eq!(failed.load(Ordering::Relaxed), 0, "of {sent}:\n{}", proksi.logs());
        assert_eq!(proksi.get("app.test", "/").await.unwrap().status, 200);
        // Each worker handed over to the next one, none failed
        for _ in 0..200 {
            if proksi.logs().matches("stopped (exit code 0)").count() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let logs = proksi.logs();
        assert_eq!(logs.matches("stopped (exit code 0)").count(), 2, "{logs}");
        assert!(!logs.contains("panicked"), "{logs}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_removed_listeners_are_closed_after_an_upgrade() {
        let mut proksi = Harness::start_with(
            vec![Route::new("app.test", Upstream::Http)],
            &format!(
                "{CONFIG}\ntcp \"other\" {{\n  listen = \"127.0.0.1:{{tcp_port_1}}\"\n  upstream \"127.0.0.1:{{http_upstream_port}}\" {{}}\n}}\n"
            ),
        )
        .await;
        assert!(tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[1]))
            .await
            .is_ok());

        proksi.rewrite_extra_config(CONFIG);
        wait_for_started(&proksi, 2).await;
        // The previous worker keeps its sockets until it stops
        wait_for_previous_to_stop(&proksi).await;

        assert!(tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[1]))
            .await
            .is_err());
        assert_eq!(proksi.get("app.test", "/").await.unwrap().status, 200);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tcp_connections_to_an_unreachable_upstream_are_closed() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // Nothing answers on this address (reserved for documentation): connecting waits
    let proksi = Harness::start_with(
        vec![],
        r#"
tcp "gone" {
  listen = "127.0.0.1:{tcp_port_0}"
  connect_timeout_secs = 1
  upstream "192.0.2.1:7878" {}
}
"#,
    )
    .await;

    let started = std::time::Instant::now();
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
        .await
        .unwrap();
    client.write_all(b"ping\n").await.unwrap();
    let mut buf = [0u8; 16];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), client.read(&mut buf)).await;

    // Closed by proksi (no data), soon after the connect timeout
    assert!(matches!(read, Ok(Ok(0) | Err(_))), "{read:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_locations_set_how_long_to_wait_for_their_upstreams() {
    // Nothing answers on this address (reserved for documentation): connecting waits
    let proksi = Harness::start_with(
        vec![],
        r#"
host "gone.test" {
  tls = false
  timeouts {
    upstream {
      connect_secs = 1
    }
  }
  location "/" {
    upstream "192.0.2.1:7878" {}
  }
}
"#,
    )
    .await;

    let started = std::time::Instant::now();
    let res = proksi.get_http_port("gone.test", "/").await.unwrap();

    assert!(res.starts_with("HTTP/1.1 504"), "{res}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_upstreams_that_dont_answer_in_time_get_a_504() {
    // Accepts connections, never answers
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent_port = silent.local_addr().unwrap().port();
    // Nothing listens on this port
    let refused_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    tokio::spawn(async move {
        let mut accepted = vec![];
        while let Ok((stream, _)) = silent.accept().await {
            accepted.push(stream);
        }
    });
    let proksi = Harness::start_with_server(
        vec![],
        "timeouts {\n upstream {\n read_secs = 1\n }\n }",
        &format!(
            r#"
host "silent.test" {{
  tls = false
  location "/" {{
    upstream "127.0.0.1:{silent_port}" {{}}
  }}
}}

host "refused.test" {{
  tls = false
  location "/" {{
    upstream "127.0.0.1:{refused_port}" {{}}
  }}
}}
"#
        ),
    )
    .await;

    let started = std::time::Instant::now();
    let res = proksi.get_http_port("silent.test", "/").await.unwrap();

    assert!(res.starts_with("HTTP/1.1 504"), "{res}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());

    // Other errors of the upstream
    let res = proksi.get_http_port("refused.test", "/").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 502"), "{res}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_idle_tcp_connections_are_closed() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let proksi = Harness::start_with(
        vec![],
        r#"
tcp "web" {
  listen = "127.0.0.1:{tcp_port_0}"
  idle_timeout_secs = 1
  upstream "127.0.0.1:{http_upstream_port}" {}
}
"#,
    )
    .await;

    // Connected to the upstream, with nothing sent either way
    let mut idle = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
        .await
        .unwrap();
    assert!(closed_within(&mut idle, std::time::Duration::from_secs(3)).await);

    // Requests every half second keep it open, past the timeout
    let mut busy = tokio::net::TcpStream::connect(("127.0.0.1", proksi.tcp_ports[0]))
        .await
        .unwrap();
    for _ in 0..5 {
        busy.write_all(b"GET /tcp HTTP/1.1\r\nHost: tcp.test\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0u8; 4096];
        let read = busy.read(&mut buf).await.unwrap();
        assert!(buf[..read].starts_with(b"HTTP/1.1 200"));
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_responses_of_proksi_itself_have_the_configured_headers() {
    // Nothing listens on this port
    let refused_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let proksi = Harness::start_with(
        vec![Route::new("app.test", Upstream::Http)],
        &format!(
            r#"
host "plain.test" {{
  tls = false
  headers {{
    response {{
      add = {{ "X-Frame-Options" = "DENY" }}
    }}
  }}
  location "/down" {{
    upstream "127.0.0.1:{refused_port}" {{}}
  }}
  location "/" {{
    rate_limit {{
      requests = 1
      per      = "1m"
    }}
    upstream "127.0.0.1:{{http_upstream_port}}" {{}}
  }}
}}
"#
        ),
    )
    .await;
    let check = |res: &str, status: &str| {
        assert!(res.starts_with(&format!("HTTP/1.1 {status}")), "{res}");
        let headers = res.split("\r\n\r\n").next().unwrap_or_default().to_lowercase();
        assert!(headers.contains("x-frame-options: deny"), "{res}");
        assert!(!headers.contains("\r\nserver:"), "{res}");
    };

    // Proxied, then answered by `rate_limit`
    check(&proksi.get_http_port("plain.test", "/").await.unwrap(), "200");
    check(&proksi.get_http_port("plain.test", "/").await.unwrap(), "429");
    // The upstream refused the connection
    check(&proksi.get_http_port("plain.test", "/down").await.unwrap(), "502");

    // A host proksi doesn't serve: no headers to add, nor a `Server`
    let res = proksi
        .tls_request(
            proksi.https_port,
            "app.test",
            "GET / HTTP/1.1\r\nHost: unknown.test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");
    assert!(!res.to_lowercase().contains("\r\nserver:"), "{res}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_upstreams_that_failed_their_health_check_get_requests_when_none_is_healthy() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // Nothing listens on this port yet: the game server is still starting
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let proksi = Harness::start_with(
        vec![],
        &format!(
            r#"
admin {{
  listen = "127.0.0.1:{{tcp_port_2}}"

  token "e2e" {{
    secret = "e2e-token"
  }}
}}

host "starting.test" {{
  tls = false
  health_check {{
    interval_secs = 1
  }}
  location "/" {{
    upstream "127.0.0.1:{port}" {{}}
  }}
}}
"#
        ),
    )
    .await;

    // Two failed checks, a second apart
    let unhealthy = format!(
        "proksi_upstream_healthy{{host=\"starting.test\",location=\"/\",upstream=\"127.0.0.1:{port}\"}} 0"
    );
    let mut metrics = String::new();
    for _ in 0..50 {
        metrics = proksi
            .raw_http_on(
                proksi.tcp_ports[2],
                b"GET /metrics HTTP/1.1\r\nHost: admin\r\nAuthorization: Bearer e2e-token\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        if metrics.lines().any(|line| line.trim_end() == unhealthy) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(metrics.lines().any(|line| line.trim_end() == unhealthy), "{metrics}");

    // Up now: requests get to it before its next check
    let upstream = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = upstream.accept().await {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await;
        }
    });
    let res = proksi.get_http_port("starting.test", "/").await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_locations_rewrite_the_path_sent_to_upstreams() {
    let mut proksi = Harness::start_with(
        vec![],
        r#"
host "rewrite.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }
  location "/api" {
    rewrite {
      strip_prefix = "/api"
      add_prefix   = "/v2"
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
  location "/static" {
    rewrite {
      strip_prefix = "/static"
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;
    proksi.resolve("rewrite.test");
    proksi.wait_for("rewrite.test", "/api", 200).await;

    for (path, upstream_path) in [
        ("/api/users?page=2", "/v2/users?page=2"),
        ("/api", "/v2"),
        ("/static/app.css", "/app.css"),
        ("/static", "/"),
        ("/static/?v=1", "/?v=1"),
    ] {
        let res = proksi.get("rewrite.test", path).await.unwrap();
        assert_eq!(res.status, 200, "{path}");
        assert_eq!(res.upstream_request()["path"], upstream_path, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_locations_rewrite_paths_with_regular_expressions() {
    let mut proksi = Harness::start_with(
        vec![],
        r#"
host "regex.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }
  location "/users" {
    rewrite {
      match   = "^/users/(?P<id>\\d+)/profile$"
      replace = "/profiles/$${id}"
    }
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;
    proksi.resolve("regex.test");
    proksi.wait_for("regex.test", "/users", 200).await;

    for (path, upstream_path) in [
        ("/users/42/profile?tab=posts", "/profiles/42?tab=posts"),
        // Not matching: unchanged
        ("/users/42", "/users/42"),
        ("/users/abc/profile", "/users/abc/profile"),
    ] {
        let res = proksi.get("regex.test", path).await.unwrap();
        assert_eq!(res.status, 200, "{path}");
        assert_eq!(res.upstream_request()["path"], upstream_path, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_unreachable_upstreams_answer_502_over_http2() {
    // Nothing listens on the port of `/down`'s upstream
    let mut proksi = Harness::start_with(
        vec![],
        r#"
host "app.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/down" {
    upstream "127.0.0.1:{tcp_port_0}" {}
  }

  location "/answered" {
    respond {
      status = 403
      body   = "no"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#,
    )
    .await;
    proksi.resolve("app.test");
    let url = |path: &str| format!("https://app.test:{}{path}", proksi.https_port);

    for client in [proksi.client(), proksi.client_h2()] {
        // Before and after a request to a working upstream, on the same connection
        for path in ["/answered", "/", "/answered", "/down", "/", "/down", "/down", "/"] {
            let res = client.get(url(path)).send().await.unwrap();
            let (version, status) = (res.version(), res.status());
            let body = res.text().await;
            assert!(body.is_ok(), "{version:?} {path} {status}: {body:?}");
            assert_eq!(status, expected(path), "{version:?} {path}");
        }
    }
    let res = proksi.client_h2().get(url("/")).send().await.unwrap();
    assert_eq!(res.version(), reqwest::Version::HTTP_2);

    proksi.assert_healthy();
}

/// The status of each path of `test_unreachable_upstreams_answer_502_over_http2`
fn expected(path: &str) -> u16 {
    match path {
        "/" => 200,
        "/answered" => 403,
        _ => 502,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_upstream_with_hostname_over_http2() {
    let proksi = Harness::start(vec![
        Route::new("app.test", Upstream::Http).upstream_ip("localhost")
    ])
    .await;
    let client = proksi.client_h2();

    // `localhost` is also ::1, where nothing listens: requests sent there are retried or 502
    for _ in 0..8 {
        let res = client
            .get(format!("https://app.test:{}/", proksi.https_port))
            .send()
            .await
            .unwrap();
        let status = res.status();
        let body = res.text().await;
        assert!(body.is_ok(), "{status}: {body:?}\n{}", proksi.logs());
        assert!(status == 200 || status == 502, "{status}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_wildcard_host_serves_one_more_label_with_its_certificate() {
    let dir = tempfile::tempdir().unwrap();
    let (pem, key) = (dir.path().join("cert.pem"), dir.path().join("key.pem"));
    let (certificate, private) = harness::signed_certificate("*.wild.test", None, false);
    std::fs::write(&pem, certificate.to_pem().unwrap()).unwrap();
    std::fs::write(&key, private.private_key_to_pem_pkcs8().unwrap()).unwrap();

    let config = format!(
        r#"
host "*.wild.test" {{
  tls {{
    certificate {{
      pem = "{}"
      key = "{}"
    }}
  }}
  location "/" {{
    upstream "127.0.0.1:{{http_upstream_port}}" {{}}
  }}
}}
"#,
        pem.display(),
        key.display()
    );
    let mut proksi = Harness::start_with(vec![], &config).await;
    for name in ["a.wild.test", "b.wild.test", "wild.test", "a.b.wild.test"] {
        proksi.resolve(name);
    }

    assert_eq!(proksi.wait_for("a.wild.test", "/", 200).await, Some(200));
    let res = proksi.get("b.wild.test", "/").await.unwrap();
    assert_eq!(res.status, 200, "{}", proksi.logs());
    assert_eq!(
        res.upstream_request()["headers"]["host"],
        format!("b.wild.test:{}", proksi.https_port)
    );

    // The wildcard host's certificate, for every name it serves
    let served = proksi
        .served_certificate(proksi.https_port, "b.wild.test")
        .await;
    assert_eq!(served.to_pem().unwrap(), certificate.to_pem().unwrap());

    // Not the parent itself, nor two more labels: unknown names, without a certificate
    assert!(proksi.get("wild.test", "/").await.is_err());
    assert!(proksi.get("a.b.wild.test", "/").await.is_err());
}

const HOST_WITH_JWT: &str = r#"
host "jwt.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    jwt {
      secret            = "e2e-secret"
      issuer            = "https://auth.test/"
      audience          = ["api"]
      claims_to_headers = { sub = "X-User-Id", roles = "X-User-Roles" }
    }
  }

  location "/cookie" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    jwt {
      secret = "e2e-secret"
      cookie = "session"
    }
  }

  location "/jwks" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    jwt {
      jwks_url = "http://127.0.0.1:{jwks_port}/jwks.json"
    }
  }
}
"#;

/// Serves `body` (a JWKS) to every request
async fn jwks_server(body: String) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn test_jwt() {
    use jsonwebtoken::{encode, jwk::Jwk, Algorithm, EncodingKey, Header};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = serde_json::json!({
        "sub": "alice", "roles": ["admin", "ops"], "iss": "https://auth.test/", "aud": "api", "exp": now + 600,
    });
    let secret = EncodingKey::from_secret(b"e2e-secret");
    let valid = encode(&Header::default(), &claims, &secret).unwrap();
    let wrong_key = encode(&Header::default(), &claims, &EncodingKey::from_secret(b"other")).unwrap();

    // An RSA key, published as a JWKS
    let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
    let rsa_key = EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
    let mut jwk = Jwk::from_encoding_key(&rsa_key, Algorithm::RS256).unwrap();
    jwk.common.key_id = Some("k1".to_string());
    // And one too short to be trusted (under 2048 bits)
    let weak = openssl::rsa::Rsa::generate(1024).unwrap();
    let weak_key = EncodingKey::from_rsa_pem(&weak.private_key_to_pem().unwrap()).unwrap();
    let mut weak_jwk = Jwk::from_encoding_key(&weak_key, Algorithm::RS256).unwrap();
    weak_jwk.common.key_id = Some("k2".to_string());
    let jwks_port =
        jwks_server(serde_json::json!({ "keys": [jwk, weak_jwk] }).to_string()).await;
    let mut jwt_header = Header::new(Algorithm::RS256);
    jwt_header.kid = Some("k1".to_string());
    let signed = encode(&jwt_header, &claims, &rsa_key).unwrap();
    jwt_header.kid = Some("k2".to_string());
    let weakly_signed = encode(&jwt_header, &claims, &weak_key).unwrap();

    let config = HOST_WITH_JWT.replace("{jwks_port}", &jwks_port.to_string());
    let mut proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let get = |path: &str, headers: &str| {
        format!("GET {path} HTTP/1.1\r\nHost: jwt.test\r\n{headers}Connection: close\r\n\r\n")
    };

    // A valid token: proxied with its claims (not the client's own headers)
    let request = get("/", &format!("Authorization: Bearer {valid}\r\nX-User-Id: mallory\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let body = res.split("\r\n\r\n").nth(1).unwrap_or_default();
    let upstream: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(upstream["headers"]["x-user-id"], "alice", "{res}");
    assert_eq!(upstream["headers"]["x-user-roles"], "admin,ops", "{res}");

    // No token, and an invalid one
    let res = proksi.raw_http_on(port, get("/", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(header(&res, "www-authenticate"), Some("Bearer"));
    let request = get("/", &format!("Authorization: Bearer {wrong_key}\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(
        header(&res, "www-authenticate"),
        Some("Bearer error=\"invalid_token\"")
    );

    // From a cookie
    let request = get("/cookie", &format!("Cookie: theme=dark; session={valid}\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let request = get("/cookie", &format!("Authorization: Bearer {valid}\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");

    // Signed by a key of the JWKS, fetched in the background
    let request = get("/jwks", &format!("Authorization: Bearer {signed}\r\n"));
    let mut status = String::new();
    for _ in 0..50 {
        let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
        status = res.lines().next().unwrap_or_default().to_string();
        if status.starts_with("HTTP/1.1 200") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(status.starts_with("HTTP/1.1 200"), "{status}\n{}", proksi.logs());
    let request = get("/jwks", &format!("Authorization: Bearer {valid}\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    let request = get("/jwks", &format!("Authorization: Bearer {weakly_signed}\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");

    proksi.assert_healthy();
}

const HOST_WITH_BODY_REWRITE: &str = r#"
host "body.test" {
  tls {
    certificate {
      pem = "{cert_pem}"
      key = "{cert_key}"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    body_rewrite {
      request {
        json {
          set    = { "$.user.role" = "guest", "$.source" = "proksi" }
          remove = ["$.debug"]
        }
        replace {
          pattern = "secret-[0-9]+"
          with    = "[redacted]"
        }
      }
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_body_rewrite() {
    let mut proksi = Harness::start_with(vec![], HOST_WITH_BODY_REWRITE).await;
    proksi.resolve("body.test");
    let url = format!("https://body.test:{}/echo-body", proksi.https_port);

    for client in [proksi.client(), proksi.client_h2()] {
        let send = |content_type: &'static str, body: String| {
            client
                .post(&url)
                .header("content-type", content_type)
                .body(body)
                .send()
        };
        let upstream = |res: reqwest::Response| async move {
            assert_eq!(res.status(), 200);
            let echoed: serde_json::Value = res.json().await.unwrap();
            // Its length isn't known when it's sent: chunked (to this HTTP/1.1 upstream)
            assert_eq!(echoed["headers"]["content-length"], serde_json::Value::Null);
            assert_eq!(echoed["headers"]["transfer-encoding"].as_str(), Some("chunked"));
            echoed["body"].as_str().unwrap().to_string()
        };

        // JSON: set, added, removed, then replaced
        let body = r#"{"user": {"id": 1, "role": "admin"}, "debug": true, "note": "secret-42"}"#;
        let sent = upstream(send("application/json", body.to_string()).await.unwrap()).await;
        let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({ "source": "proksi", "user": { "id": 1, "role": "guest" }, "note": "[redacted]" })
        );

        // Another content type: only the replacements
        let sent = upstream(send("text/plain", "token secret-7, debug".to_string()).await.unwrap()).await;
        assert_eq!(sent, "token [redacted], debug");

        // Larger than a part, changed as it streams
        let items = (0..20_000).map(|i| format!("{{\"id\":{i}}}")).collect::<Vec<_>>().join(",");
        let body = format!(r#"{{"debug": 1, "items": [{items}]}}"#);
        assert!(body.len() > 200_000);
        let sent = upstream(send("application/json", body).await.unwrap()).await;
        let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
        assert_eq!(sent["items"].as_array().map(Vec::len), Some(20_000));
        assert_eq!(sent.get("debug"), None);
        assert_eq!(sent["source"], "proksi");

        // Compressed, or not text: sent as it is, with its length
        for (content_type, encoding) in [("application/json", "gzip"), ("application/octet-stream", "identity")] {
            let request = client.post(&url).header("content-type", content_type).header("content-encoding", encoding);
            let res = request.body("secret-1 {\"debug\": true}").send().await.unwrap();
            let echoed: serde_json::Value = res.json().await.unwrap();
            assert_eq!(echoed["body"], "secret-1 {\"debug\": true}", "{content_type}");
            assert_eq!(echoed["headers"]["content-length"], "24", "{content_type}");
        }

        // A body nothing applies to is sent as it is, with its length
        let res = client.post(&url).header("content-type", "text/plain").body("nothing here").send().await.unwrap();
        let echoed: serde_json::Value = res.json().await.unwrap();
        assert_eq!(echoed["body"], "nothing here");
    }

    proksi.assert_healthy();
}

const HOST_WITH_WAITING_ROOM: &str = r#"
host "line.test" {
  tls = false

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    waiting_room {
      active         = 1
      session        = "3s"
      refresh        = "1s"
      on_store_error = "wait"
    }
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_waiting_room() {
    let mut harness = Harness::start_without_https(HOST_WITH_WAITING_ROOM).await;
    let port = harness.http_port;
    let proksi = &harness;
    let get = |cookie: Option<String>| async move {
        let cookie = cookie.map(|cookie| format!("Cookie: {cookie}\r\n")).unwrap_or_default();
        let request = format!("GET / HTTP/1.1\r\nHost: line.test\r\n{cookie}Connection: close\r\n\r\n");
        proksi.raw_http_on(port, request.as_bytes()).await.unwrap()
    };
    let status = |res: &str| res.split(' ').nth(1).unwrap_or_default().to_string();
    // The cookie a browser sends back
    let pass = |res: &str| {
        header(res, "set-cookie").map(|cookie| cookie.split(';').next().unwrap().to_string())
    };

    // The first visitor is let in, with a pass
    let res = get(None).await;
    assert_eq!(status(&res), "200", "{res}");
    let first = pass(&res).unwrap();
    assert!(first.starts_with("proksi_room_"), "{first}");

    // The second waits, first in line, on a page that doesn't get cached
    let res = get(None).await;
    assert_eq!(status(&res), "503", "{res}");
    assert_eq!(header(&res, "retry-after"), Some("1"));
    assert_eq!(header(&res, "cache-control"), Some("no-store"));
    assert!(res.contains("<div class=\"place\">1</div>"), "{res}");
    let second = pass(&res).unwrap();

    // The first goes on with their pass, without a new one
    let res = get(Some(first.clone())).await;
    assert_eq!(status(&res), "200", "{res}");
    assert_eq!(pass(&res), None);
    // A pass that was changed is no pass: a new visitor, in line
    let mut changed = first.clone();
    changed.pop();
    changed.push(if first.ends_with('A') { 'B' } else { 'A' });
    assert_eq!(status(&get(Some(changed)).await), "503");

    // The second keeps their place by refreshing, and is let in once the first's slot expires
    let start = std::time::Instant::now();
    let mut admitted = None;
    while start.elapsed() < std::time::Duration::from_secs(8) {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let res = get(Some(second.clone())).await;
        if status(&res) == "200" {
            admitted = Some(start.elapsed());
            assert!(pass(&res).is_some(), "{res}");
            break;
        }
    }
    let admitted = admitted.expect("the second visitor was never let in");
    assert!(admitted >= std::time::Duration::from_secs(2), "{admitted:?}");

    // Now the first waits
    assert_eq!(status(&get(Some(first)).await), "503");

    harness.assert_healthy();
}

const HOST_WITH_BANDWIDTH_LIMIT: &str = r#"
host "paced.test" {
  tls = false

  location "/api" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    bandwidth_limit {
      rate  = "4KiB"
      burst = "1KiB"
    }
  }

  location "/files" {
    static_files {
      directory = "{files}"
    }

    bandwidth_limit {
      rate  = "128KiB"
      burst = "64KiB"
    }
  }

  location "/" {
    upstream "127.0.0.1:{http_upstream_port}" {}
  }
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn test_bandwidth_limit() {
    let files = tempfile::tempdir().unwrap();
    std::fs::write(files.path().join("big.bin"), vec![b'x'; 256 * 1024]).unwrap();
    let config =
        HOST_WITH_BANDWIDTH_LIMIT.replace("{files}", &files.path().display().to_string());
    let mut harness = Harness::start_without_https(&config).await;
    let port = harness.http_port;
    // The upstream answers with the request's headers: about 8 KiB with this one
    let padded = |path: &str| {
        format!(
            "GET {path} HTTP/1.1\r\nHost: paced.test\r\nX-Pad: {}\r\nConnection: close\r\n\r\n",
            "p".repeat(8000)
        )
    };
    let proksi = &harness;
    let timed = |request: String| async move {
        let start = std::time::Instant::now();
        let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
        (res, start.elapsed())
    };

    // An upstream's answer: 1 KiB at once, the rest at 4 KiB/s
    let (res, paced) = timed(padded("/api/a")).await;
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(res.contains(&"p".repeat(8000)));
    assert!(paced >= std::time::Duration::from_millis(1200), "{paced:?}");
    // Other locations aren't paced
    let (_, unpaced) = timed(padded("/b")).await;
    assert!(unpaced < std::time::Duration::from_millis(500), "{unpaced:?}");

    // A file: 64 KiB at once, the other 192 KiB at 128 KiB/s
    let file = "GET /files/big.bin HTTP/1.1\r\nHost: paced.test\r\nConnection: close\r\n\r\n";
    let (res, paced) = timed(file.to_string()).await;
    assert!(res.starts_with("HTTP/1.1 200"), "{}", &res[..res.len().min(200)]);
    assert!(res.ends_with(&"x".repeat(1024)));
    assert!(paced >= std::time::Duration::from_millis(1000), "{paced:?}");

    harness.assert_healthy();
}

#[tokio::test(flavor = "multi_thread")]
async fn test_static_files() {
    let site = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(site.path().join("app")).unwrap();
    std::fs::create_dir_all(site.path().join("docs")).unwrap();
    std::fs::write(site.path().join("app/index.html"), "<h1>app</h1>").unwrap();
    std::fs::write(site.path().join("app/app.js"), "console.log(1)").unwrap();
    std::fs::write(site.path().join("app/.env"), "SECRET=1").unwrap();
    std::fs::write(site.path().join("secret.txt"), "outside").unwrap();
    // Bigger than a chunk (64 KiB), to be streamed
    let big = "0123456789".repeat(20_000);
    std::fs::write(site.path().join("app/big.txt"), &big).unwrap();

    let config = format!(
        r#"
host "files.test" {{
  tls = false

  location "/static" {{
    static_files {{
      directory     = "{}"
      fallback      = "/index.html"
      cache_control = "public, max-age=60"
    }}

    rule "no maps" {{
      and {{
        path {{
          suffix = ".map"
        }}
      }}
      then {{
        respond {{
          status = 403
        }}
      }}
    }}
  }}

  location "/private" {{
    static_files {{
      directory = "{}"
    }}
    basic_auth {{
      user = "u"
      pass = "p"
    }}
  }}
}}
"#,
        site.path().join("app").display(),
        site.path().join("app").display()
    );
    let mut proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let send = |method: &str, path: &str, headers: &str| {
        format!("{method} {path} HTTP/1.1\r\nHost: files.test\r\n{headers}Connection: close\r\n\r\n")
    };
    let body = |res: &str| res.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default();

    // A file, by its path below the location
    let res = proksi.raw_http_on(port, send("GET", "/static/app.js", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(body(&res), "console.log(1)");
    assert_eq!(header(&res, "content-type"), Some("text/javascript; charset=utf-8"));
    assert_eq!(header(&res, "cache-control"), Some("public, max-age=60"));
    let etag = header(&res, "etag").unwrap();

    // Without its body, and not again while it's unchanged
    let res = proksi.raw_http_on(port, send("HEAD", "/static/app.js", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(header(&res, "content-length"), Some("14"));
    assert_eq!(body(&res), "");
    let request = send("GET", "/static/app.js", &format!("If-None-Match: {etag}\r\n"));
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 304"), "{res}");

    // A part of it
    let request = send("GET", "/static/app.js", "Range: bytes=0-6\r\n");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 206"), "{res}");
    assert_eq!(body(&res), "console");
    assert_eq!(header(&res, "content-range"), Some("bytes 0-6/14"));

    // The index of a directory, and the fallback for paths with no file
    for path in ["/static", "/static/", "/static/settings/profile"] {
        let res = proksi.raw_http_on(port, send("GET", path, "").as_bytes()).await.unwrap();
        assert!(res.starts_with("HTTP/1.1 200"), "{path}: {res}");
        assert_eq!(body(&res), "<h1>app</h1>", "{path}");
    }

    // Streamed, whole
    let res = proksi.raw_http_on(port, send("GET", "/static/big.txt", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{}", &res[..200]);
    assert_eq!(body(&res), big);

    // Nothing outside the directory, nor hidden files
    for path in ["/static/../secret.txt", "/static/%2e%2e/secret.txt", "/static/.env"] {
        let res = proksi.raw_http_on(port, send("GET", path, "").as_bytes()).await.unwrap();
        assert!(!res.contains("outside") && !res.contains("SECRET"), "{path}: {res}");
    }
    let res = proksi.raw_http_on(port, send("GET", "/static/.env", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");

    // Rules and the other blocks apply before files are served
    std::fs::write(site.path().join("app/app.js.map"), "{}").unwrap();
    let res = proksi.raw_http_on(port, send("GET", "/static/app.js.map", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 403"), "{res}");
    let res = proksi.raw_http_on(port, send("GET", "/private/app.js", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    let request = send("GET", "/private/app.js", "Authorization: Basic dTpw\r\n");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(body(&res), "console.log(1)");

    // Only read
    let res = proksi.raw_http_on(port, send("POST", "/static/app.js", "Content-Length: 0\r\n").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 405"), "{res}");
    assert_eq!(header(&res, "allow"), Some("GET, HEAD"));

    proksi.assert_healthy();
}

/// An upstream answering `body` to every request
async fn answering(body: &'static str) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn test_traffic_split() {
    let v2_port = answering("v2").await;
    let config = r#"
host "split.test" {
  tls = false

  location "/all" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    traffic_split "v2" {
      percent = 100
      upstream "127.0.0.1:{v2_port}" {}
    }
  }

  location "/beta" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    traffic_split "v2" {
      percent = 0
      upstream "127.0.0.1:{v2_port}" {}
    }

    rule "beta testers" {
      and {
        header "x-beta" {
          exists = true
        }
      }
      then {
        traffic_split = "v2"
      }
    }
  }

  location "/down" {
    upstream "127.0.0.1:{http_upstream_port}" {}
    health_check {
      interval_secs = 1
    }

    traffic_split "v2" {
      percent = 100
      sticky  = "none"
      upstream "127.0.0.1:1" {}
    }
  }
}
"#
    .replace("{v2_port}", &v2_port.to_string());
    let mut proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let get = |path: &str, headers: &str| {
        format!("GET {path} HTTP/1.1\r\nHost: split.test\r\n{headers}Connection: close\r\n\r\n")
    };
    let body = |res: &str| res.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default();

    // A new client gets the split, and a cookie telling nothing to keep it
    let res = proksi.raw_http_on(port, get("/all", "").as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(body(&res), "v2");
    let cookie = header(&res, "set-cookie").unwrap();
    assert!(cookie.starts_with("_t") && cookie.contains("; HttpOnly"), "{cookie}");
    assert!(!cookie.to_lowercase().contains("split") && !cookie.contains("v2"), "{cookie}");
    let pair = cookie.split(';').next().unwrap().to_string();

    // With it, the same side, and no new cookie
    let res = proksi.raw_http_on(port, get("/all", &format!("Cookie: {pair}\r\n")).as_bytes()).await.unwrap();
    assert_eq!(body(&res), "v2");
    assert_eq!(header(&res, "set-cookie"), None);

    // At 0%, only the clients a rule sends there
    let res = proksi.raw_http_on(port, get("/beta", "").as_bytes()).await.unwrap();
    assert_ne!(body(&res), "v2", "{res}");
    let res = proksi.raw_http_on(port, get("/beta", "X-Beta: 1\r\n").as_bytes()).await.unwrap();
    assert_eq!(body(&res), "v2");

    // A split whose upstreams the health checks found down: the location's own
    let mut res = String::new();
    for _ in 0..50 {
        res = proksi.raw_http_on(port, get("/down", "").as_bytes()).await.unwrap();
        if res.starts_with("HTTP/1.1 200") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(res.starts_with("HTTP/1.1 200"), "{res}\n{}", proksi.logs());
    assert_ne!(body(&res), "v2");

    proksi.assert_healthy();
}

/// A header of a raw HTTP response, by name in any case
fn header<'a>(res: &'a str, name: &str) -> Option<&'a str> {
    res.lines()
        .take_while(|line| !line.is_empty())
        .find_map(|line| {
            let (key, value) = line.split_once(": ")?;
            key.eq_ignore_ascii_case(name).then_some(value)
        })
}

/// A request's head (up to its blank line), and the part of its body read with it; `None` when
/// the connection ends first
async fn read_head(stream: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    use tokio::io::AsyncReadExt;
    let mut data = vec![];
    let mut buffer = [0u8; 4096];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .ok()
            .filter(|read| *read > 0)?;
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&data[..end]).to_string();
            return Some((head, data[end + 4..].to_vec()));
        }
    }
}

/// The rest of a body of `length` bytes, until the connection ends; `None` when reading fails
async fn read_body(
    stream: &mut tokio::net::TcpStream,
    body: &mut Vec<u8>,
    length: usize,
) -> Option<()> {
    use tokio::io::AsyncReadExt;
    let mut buffer = [0u8; 4096];
    while body.len() < length {
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    Some(())
}

/// An upstream keeping the requests it gets: request line, headers (lowercase) and body
async fn recording() -> (u16, Arc<Mutex<Vec<(String, Vec<(String, String)>, String)>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(vec![]));
    let kept = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let kept = kept.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let Some((head, mut body)) = read_head(&mut stream).await else {
                    return;
                };
                let mut lines = head.lines();
                let line = lines.next().unwrap_or_default().to_string();
                let headers = lines
                    .filter_map(|line| line.split_once(": "))
                    .map(|(name, value)| (name.to_lowercase(), value.to_string()))
                    .collect::<Vec<_>>();
                let length = headers
                    .iter()
                    .find(|(name, _)| name == "content-length")
                    .and_then(|(_, value)| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if read_body(&mut stream, &mut body, length).await.is_none() {
                    return;
                }
                kept.lock().unwrap().push((
                    line,
                    headers,
                    String::from_utf8_lossy(&body).to_string(),
                ));
                let _ = stream
                    .write_all(b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    (port, requests)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_mirror() {
    let (mirror_port, mirrored) = recording().await;
    let config = r#"
host "mirror.test" {
  tls = false

  location "/all" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    mirror {
      max_body_size = "16B"
      upstream "127.0.0.1:{mirror_port}" {}
    }

    rule "private" {
      and {
        header "x-private" {
          exists = true
        }
      }
      then {
        mirror = false
      }
    }
  }

  location "/chosen" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    mirror {
      percent = 0
      upstream "127.0.0.1:{mirror_port}" {}
    }

    rule "shadow" {
      and {
        header "x-shadow" {
          exists = true
        }
      }
      then {
        mirror = true
      }
    }
  }

  location "/down" {
    upstream "127.0.0.1:{http_upstream_port}" {}

    mirror {
      upstream "127.0.0.1:1" {}
    }
  }
}
"#
    .replace("{mirror_port}", &mirror_port.to_string());
    let mut proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let send = |method: &str, path: &str, headers: &str, body: &str| {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: mirror.test\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    // The paths mirrored, once `path` was
    let mirrored_until = |path: &'static str| {
        let mirrored = mirrored.clone();
        async move {
            for _ in 0..50 {
                let paths = mirrored
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(line, _, _)| line.split(' ').nth(1).unwrap_or_default().to_string())
                    .collect::<Vec<_>>();
                if paths.iter().any(|seen| seen == path) {
                    return paths;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("{path} wasn't mirrored");
        }
    };

    // A copy of the request, as proxied: method, path, headers and body
    let request = send("POST", "/all/orders?id=1", "X-Client: 7\r\n", "hello");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    mirrored_until("/all/orders?id=1").await;
    {
        let mirrored = mirrored.lock().unwrap();
        let (line, headers, body) = &mirrored[0];
        assert_eq!(line, "POST /all/orders?id=1 HTTP/1.1");
        assert_eq!(body, "hello");
        let header = |name: &str| {
            headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(header("host"), Some("mirror.test"));
        assert_eq!(header("x-client"), Some("7"));
        assert_eq!(header("x-forwarded-for"), Some("127.0.0.1"));
    }

    // Not copied: bodies over max_body_size, and requests a rule keeps
    let request = send("POST", "/all/large", "", "a body of more than 16 bytes");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let request = send("GET", "/all/private", "X-Private: 1\r\n", "");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    // At 0%, only the requests a rule copies
    let request = send("GET", "/chosen/plain", "", "");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let request = send("GET", "/chosen/shadow", "X-Shadow: 1\r\n", "");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");

    let paths = mirrored_until("/chosen/shadow").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let paths_now = mirrored
        .lock()
        .unwrap()
        .iter()
        .map(|(line, _, _)| line.split(' ').nth(1).unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), paths_now.len(), "{paths_now:?}");
    for path in ["/all/large", "/all/private", "/chosen/plain"] {
        assert!(
            !paths_now.iter().any(|seen| seen == path),
            "{path}: {paths_now:?}"
        );
    }

    // A mirror that can't be reached changes nothing for the requests
    let started = std::time::Instant::now();
    let request = send("POST", "/down", "", "hello");
    let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(started.elapsed() < Duration::from_secs(2));

    proksi.assert_healthy();
}

/// An upstream answering `status` with `body` after `delay`, counting the requests it gets
async fn answering_after(
    status: u16,
    body: &'static str,
    delay: Duration,
) -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = count.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let counted = counted.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tokio::time::sleep(delay).await;
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (port, count)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_retries() {
    let (failing, _) = answering_after(502, "failing", Duration::ZERO).await;
    let (working, _) = answering_after(200, "working", Duration::ZERO).await;
    let config = r#"
host "retry.test" {
  tls = false

  location "/status" {
    upstream "127.0.0.1:{failing}" {}
    upstream "127.0.0.1:{working}" {}

    retry {
      on = ["502"]
    }

    rule "no retries" {
      and {
        header "x-no-retry" {
          exists = true
        }
      }
      then {
        retry = false
      }
    }
  }

  location "/connect" {
    upstream "127.0.0.1:1" {}
    upstream "127.0.0.1:{working}" {}
    health_check {
      enabled = false
    }

    retry {}
  }
}
"#
    .replace("{failing}", &failing.to_string())
    .replace("{working}", &working.to_string());
    let proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let send = |method: &str, path: &str, headers: &str| {
        format!("{method} {path} HTTP/1.1\r\nHost: retry.test\r\n{headers}Content-Length: 0\r\nConnection: close\r\n\r\n")
    };
    let statuses = |method: &'static str, path: &'static str, headers: &'static str| {
        let request = send(method, path, headers);
        let proksi = &proksi;
        async move {
            let mut statuses = vec![];
            // Round robin: both upstreams, whichever comes first
            for _ in 0..4 {
                let res = proksi.raw_http_on(port, request.as_bytes()).await.unwrap();
                statuses.push(res.split(' ').nth(1).unwrap_or_default().to_string());
            }
            statuses
        }
    };

    // A 502: tried again on the other upstream, for idempotent requests
    assert_eq!(statuses("GET", "/status", "").await, ["200"; 4]);
    assert!(statuses("POST", "/status", "")
        .await
        .contains(&"502".to_string()));
    // Unless a rule says otherwise
    assert!(statuses("GET", "/status", "X-No-Retry: 1\r\n")
        .await
        .contains(&"502".to_string()));

    // An upstream that can't be reached: tried again, whatever the method (nothing was sent)
    assert_eq!(statuses("GET", "/connect", "").await, ["200"; 4]);
    assert_eq!(statuses("POST", "/connect", "").await, ["200"; 4]);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_least_connections() {
    let (slow, slow_count) = answering_after(200, "slow", Duration::from_millis(1500)).await;
    let (fast, fast_count) = answering_after(200, "fast", Duration::ZERO).await;
    let config = r#"
host "least.test" {
  tls = false

  location "/" {
    load_balancing = "least_connections"
    upstream "127.0.0.1:{slow}" {}
    upstream "127.0.0.1:{fast}" {}
  }
}
"#
    .replace("{slow}", &slow.to_string())
    .replace("{fast}", &fast.to_string());
    let proksi = Harness::start_without_https(&config).await;
    let port = proksi.http_port;
    let request = "GET / HTTP/1.1\r\nHost: least.test\r\nConnection: close\r\n\r\n";

    // While the slow upstream holds a request, the fast one takes the others
    let mut requests = vec![];
    for _ in 0..20 {
        requests.push(tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).await.unwrap();
            res
        }));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for res in futures_util::future::join_all(requests).await {
        assert!(res.unwrap().starts_with("HTTP/1.1 200"));
    }
    let (slow, fast) = (
        slow_count.load(std::sync::atomic::Ordering::Relaxed),
        fast_count.load(std::sync::atomic::Ordering::Relaxed),
    );
    // Round robin would have sent it 10
    assert!(slow <= 2 && slow + fast == 20, "slow {slow}, fast {fast}");
}
