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

    let res = proksi
        .raw_http_on(admin_port, metrics(Some("e2e-token")).as_bytes())
        .await
        .unwrap();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
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
