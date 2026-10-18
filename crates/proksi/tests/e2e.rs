//! End-to-end tests running the `proksi` binary against local upstreams.
//!
//! See `harness/mod.rs` for how proksi and the upstreams are started.

mod harness;

use std::time::Duration;

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
        assert_eq!(fields["peer_addr"], upstream.as_str(), "{log}");
        // The connection may come from proksi's readiness checks
        assert!(
            ["true", "false"].contains(&fields["reused_connection"].as_str().unwrap_or("")),
            "{log}"
        );
        assert_eq!(fields["request_id"], request_id.as_str(), "{log}");
    }

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
    let encoding = |res: &str| {
        res.lines()
            .take_while(|line| !line.is_empty())
            .find_map(|line| {
                let line = line.to_lowercase();
                line.strip_prefix("content-encoding: ").map(str::to_string)
            })
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
        assert_eq!(encoding(&res).as_deref(), expected, "{path} {accept}: {res}");
    }

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
  token  = "e2e-token"
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
        r#"{{ "tls": {{ "enabled": false }}, "locations": [{{ "path": "/", "upstreams": [{{ "ip": "127.0.0.1", "port": {} }}] }}] }}"#,
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

    // HCL, like a host block of the configuration file
    let hcl = format!(
        "tls = false\nlocation \"/\" {{\n  rate_limit {{\n    requests = 1\n    per = \"1m\"\n  }}\n  upstream \"127.0.0.1:{}\" {{}}\n}}\n",
        proksi.http_upstream.port
    );
    let res = proksi
        .raw_http_on(
            admin_port,
            request("PUT", "/api/v1/routes/api.test", "text/hcl", &hcl).as_bytes(),
        )
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    served("HTTP/1.1 429").await;

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
  token   = "e2e-token"
  metrics = false
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
  token          = "admin-token"
  metrics_listen = "127.0.0.1:{tcp_port_1}"
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
    // Nothing listens on this port
    let refused_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
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
    let _ = client.read(&mut buf).await;

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
  token  = "e2e-token"
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
