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
    assert_eq!(proksi.wait_for("a.test", "/", 404).await, Some(404));
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
