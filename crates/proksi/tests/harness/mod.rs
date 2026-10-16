//! End-to-end test harness.
//!
//! Starts the real `proksi` binary with a generated configuration, in front of
//! in-process upstreams (an HTTP echo server and a websocket echo server).
#![allow(dead_code)]

use std::{
    collections::HashMap,
    fmt::Write as _,
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use openssl::{
    asn1::Asn1Time,
    bn::{BigNum, MsbOption},
    hash::MessageDigest,
    pkey::PKey,
    rsa::Rsa,
    x509::{extension::SubjectAlternativeName, X509NameBuilder, X509},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{tungstenite::Message, Connector, MaybeTlsStream, WebSocketStream};

/// Host that is always routed to the HTTP upstream, used to know when proksi is ready
const READY_HOST: &str = "ready.test";

/// Header added by the HTTP upstream when the path contains `utf8`
pub const UTF8_CONTENT_DISPOSITION: &str = "attachment; filename=\"résumé.pdf\"";

pub type WebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Copy)]
pub enum Upstream {
    Http,
    WebSocket,
}

/// A route to configure in proksi
pub struct Route {
    host: String,
    upstream: Upstream,
    upstream_ip: String,
    paths: Vec<String>,
    cache: bool,
}

impl Route {
    pub fn new(host: &str, upstream: Upstream) -> Self {
        Self {
            host: host.to_string(),
            upstream,
            upstream_ip: "127.0.0.1".to_string(),
            paths: vec![],
            cache: false,
        }
    }

    /// Only match the given path patterns
    pub fn paths(mut self, paths: &[&str]) -> Self {
        self.paths = paths.iter().map(ToString::to_string).collect();
        self
    }

    /// Enables the disk cache for this route
    pub fn cache(mut self) -> Self {
        self.cache = true;
        self
    }

    /// Uses a hostname (instead of an IP) for the upstream
    pub fn upstream_ip(mut self, ip: &str) -> Self {
        self.upstream_ip = ip.to_string();
        self
    }
}

/// HTTP response as seen by the client
pub struct Response {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The request as received by the HTTP upstream
    pub fn upstream_request(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_default()
    }
}

/// A running proksi instance and its upstreams
pub struct Harness {
    child: Child,
    dir: tempfile::TempDir,
    hosts: Vec<String>,
    /// Extra configuration added to the generated one (e.g. `auto_reload`)
    extra_config: String,
    /// Extra attributes of the generated `server` block
    server_config: String,
    /// Started with only the hosts of `extra_config` (see `start_without_https`)
    without_https: bool,
    pub https_port: u16,
    pub http_port: u16,
    /// A free port for hosts listening elsewhere (`{extra_port}` in `extra_config`)
    pub extra_port: u16,
    /// Free ports for `tcp` blocks (`{tcp_port_0}`... in `extra_config`)
    pub tcp_ports: [u16; 3],
    pub http_upstream: HttpUpstream,
    pub websocket_upstream: WebSocketUpstream,
}

impl Harness {
    pub async fn start(routes: Vec<Route>) -> Self {
        Self::start_with(routes, "").await
    }

    /// Starts proksi with extra configuration (e.g. an `auto_reload` block)
    pub async fn start_with(routes: Vec<Route>, extra_config: &str) -> Self {
        Self::start_inner(routes, "", extra_config, false).await
    }

    /// Starts proksi with only the hosts of `extra_config`, all with `tls = false`:
    /// no host needs HTTPS, so proksi doesn't open the HTTPS port.
    /// Readiness is checked on the HTTP port.
    pub async fn start_without_https(extra_config: &str) -> Self {
        Self::start_inner(vec![], "", extra_config, true).await
    }

    /// Like `start_with`, with extra attributes in the `server` block
    pub async fn start_with_server(
        routes: Vec<Route>,
        server_config: &str,
        extra_config: &str,
    ) -> Self {
        Self::start_inner(routes, server_config, extra_config, false).await
    }

    async fn start_inner(
        routes: Vec<Route>,
        server_config: &str,
        extra_config: &str,
        without_https: bool,
    ) -> Self {
        raise_open_files_limit();

        let dir = tempfile::tempdir().unwrap();
        write_self_signed_cert(dir.path());

        let http_upstream = HttpUpstream::start().await;
        let websocket_upstream = WebSocketUpstream::start().await;

        let https_port = free_port();
        let http_port = free_port();
        let extra_port = free_port();
        let tcp_ports = [free_port(), free_port(), free_port()];

        let stdout = std::fs::File::create(dir.path().join("stdout.log")).unwrap();
        let stderr = std::fs::File::create(dir.path().join("stderr.log")).unwrap();
        // Replaced once the configuration is written
        let placeholder = Command::new("true").spawn().unwrap();

        let mut harness = Self {
            child: placeholder,
            dir,
            hosts: vec![],
            extra_config: extra_config.to_string(),
            server_config: server_config.to_string(),
            without_https,
            https_port,
            http_port,
            extra_port,
            tcp_ports,
            http_upstream,
            websocket_upstream,
        };
        harness.rewrite_routes(routes);

        // `PROKSI_BIN` allows running the tests against another build (e.g. a release binary)
        let bin = std::env::var("PROKSI_BIN").unwrap_or(env!("CARGO_BIN_EXE_proksi").to_string());
        harness.child = Command::new(bin)
            .arg("-c")
            .arg(harness.config_path())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap();

        harness.wait_until_ready().await;
        harness
    }

    pub fn config_path(&self) -> PathBuf {
        self.dir.path().join("proksi.hcl")
    }

    /// Writes the configuration file with the given routes
    pub fn rewrite_routes(&mut self, routes: Vec<Route>) {
        let mut routes = routes;
        routes.push(Route::new(READY_HOST, Upstream::Http));
        // Hosts of removed routes are kept, so that the client still resolves them
        for route in &routes {
            if !self.hosts.contains(&route.host) {
                self.hosts.push(route.host.clone());
            }
        }

        let config = self.render_config(&routes);
        // Written to another file first, so proksi never reads a partial file
        let tmp_path = self.dir.path().join("proksi.hcl.tmp");
        std::fs::write(&tmp_path, config).unwrap();
        std::fs::rename(&tmp_path, self.config_path()).unwrap();
    }

    /// Replaces the configuration file with arbitrary content
    pub fn write_config(&self, content: &str) {
        std::fs::write(self.config_path(), content).unwrap();
    }

    fn render_config(&self, routes: &[Route]) -> String {
        let dir = self.dir.path();
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");

        let mut routes_hcl = String::new();
        for route in routes {
            let upstream_port = match route.upstream {
                Upstream::Http => self.http_upstream.port,
                Upstream::WebSocket => self.websocket_upstream.port,
            };
            let _ = writeln!(routes_hcl, "  {{");
            let _ = writeln!(routes_hcl, "    host = \"{}\"", route.host);
            let _ = writeln!(
                routes_hcl,
                "    ssl = {{ path = {{ key = \"{}\", pem = \"{}\" }} }}",
                key.to_string_lossy(),
                cert.to_string_lossy()
            );
            let _ = writeln!(
                routes_hcl,
                "    upstreams = [{{ ip = \"{}\", port = {upstream_port} }}]",
                route.upstream_ip
            );
            if !route.paths.is_empty() {
                let patterns = route
                    .paths
                    .iter()
                    .map(|p| format!("\"{p}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    routes_hcl,
                    "    match_with = {{ path = {{ patterns = [{patterns}] }} }}"
                );
            }
            if route.cache {
                let cache_dir = dir.join("cache");
                let _ = writeln!(
                    routes_hcl,
                    "    cache = {{ enabled = true, cache_type = \"disk\", expires_in_secs = 60, path = \"{}\" }}",
                    cache_dir.to_string_lossy()
                );
            }
            let _ = writeln!(routes_hcl, "  }},");
        }

        format!(
            r#"
service_name = "proksi-e2e"
worker_threads = 2

server {{
  https_address = "127.0.0.1:{https_port}"
  http_address = "127.0.0.1:{http_port}"
  {server_config}
}}

logging {{
  enabled = true
  level = "info"
  format = "json"
}}

lets_encrypt {{
  enabled = false
  email = "e2e@proksi.test"
}}

paths {{
  lets_encrypt = "{le}"
}}

{extra_config}

{hosts}
"#,
            https_port = self.https_port,
            http_port = self.http_port,
            server_config = self.server_config,
            hosts = self.render_hosts(&routes_hcl),
            le = dir.join("le").to_string_lossy(),
            extra_config = self.render_extra_config(),
        )
    }

    /// The routes, or without HTTPS, the readiness host over plain HTTP
    fn render_hosts(&self, routes_hcl: &str) -> String {
        if !self.without_https {
            return format!("routes = [\n{routes_hcl}]");
        }
        format!(
            "host \"{READY_HOST}\" {{\n  tls = false\n  location \"/\" {{\n    upstream \"127.0.0.1:{}\" {{}}\n  }}\n}}",
            self.http_upstream.port
        )
    }

    /// Extra configuration, with `{http_upstream_port}`, the ports of proksi, `{cert_pem}` and
    /// `{cert_key}` replaced
    fn render_extra_config(&self) -> String {
        let dir = self.dir.path();
        let mut config = self.extra_config.clone();
        for (index, port) in self.tcp_ports.iter().enumerate() {
            config = config.replace(&format!("{{tcp_port_{index}}}"), &port.to_string());
        }
        config
            .replace("{http_upstream_port}", &self.http_upstream.port.to_string())
            .replace("{https_port}", &self.https_port.to_string())
            .replace("{http_port}", &self.http_port.to_string())
            .replace("{extra_port}", &self.extra_port.to_string())
            .replace("{cert_pem}", &dir.join("cert.pem").to_string_lossy())
            .replace("{cert_key}", &dir.join("key.pem").to_string_lossy())
    }

    /// Makes the client resolve `host` to proksi (for hosts defined in `extra_config`)
    pub fn resolve(&mut self, host: &str) {
        self.hosts.push(host.to_string());
    }

    /// Waits until a request returns the given status (or 10 seconds), returning the last status
    pub async fn wait_for(&self, host: &str, path: &str, status: u16) -> Option<u16> {
        let mut last = None;
        for _ in 0..50 {
            last = self.get(host, path).await.ok().map(|res| res.status);
            if last == Some(status) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        last
    }

    async fn wait_until_ready(&mut self) {
        for _ in 0..100 {
            assert!(self.is_alive(), "proksi exited:\n{}", self.logs());

            let ready = if self.without_https {
                self.get_http_port(READY_HOST, "/")
                    .await
                    .is_ok_and(|res| res.starts_with("HTTP/1.1 200"))
            } else {
                self.get(READY_HOST, "/")
                    .await
                    .is_ok_and(|res| res.status == 200)
            };
            if ready {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        panic!("proksi did not become ready:\n{}", self.logs());
    }

    /// HTTPS client resolving every configured host to proksi
    pub fn client(&self) -> reqwest::Client {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.https_port));
        let mut builder = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .resolve("unknown.test", addr);
        for host in &self.hosts {
            builder = builder.resolve(host, addr);
        }
        builder.build().unwrap()
    }

    pub async fn get(&self, host: &str, path: &str) -> reqwest::Result<Response> {
        let url = format!("https://{host}:{}{path}", self.https_port);
        let res = self
            .client()
            .get(url)
            .header("x-custom", "e2e")
            .send()
            .await?;

        Ok(Response {
            status: res.status().as_u16(),
            headers: res.headers().clone(),
            body: res.text().await?,
        })
    }

    /// Sends a request to the plain HTTP port with a raw `Host` header
    pub async fn get_http_port(&self, host: &str, path: &str) -> std::io::Result<String> {
        self.get_http_on(self.http_port, host, path).await
    }

    /// Plain HTTP request to proksi on `port`, returning the raw response
    pub async fn get_http_on(&self, port: u16, host: &str, path: &str) -> std::io::Result<String> {
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        self.raw_http_on(port, request.as_bytes()).await
    }

    /// Sends `request` as is to proksi on `port` (plain HTTP), returning the raw response
    pub async fn raw_http_on(&self, port: u16, request: &[u8]) -> std::io::Result<String> {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        stream.write_all(request).await?;

        let mut response = String::new();
        tokio::time::timeout(
            Duration::from_secs(10),
            stream.read_to_string(&mut response),
        )
        .await??;
        Ok(response)
    }

    /// Sends `request` over TLS to proksi on `port`, asking for `server_name` (SNI),
    /// returning the raw response
    pub async fn tls_request(
        &self,
        port: u16,
        server_name: &str,
        request: &str,
    ) -> std::io::Result<String> {
        self.tls_request_with_alpn(port, server_name, &[], request)
            .await
    }

    /// Like `tls_request`, offering the protocols `alpn`
    pub async fn tls_request_with_alpn(
        &self,
        port: u16,
        server_name: &str,
        alpn: &[&str],
        request: &str,
    ) -> std::io::Result<String> {
        let (server_name, request) = (server_name.to_string(), request.to_string());
        let alpn = alpn.iter().map(ToString::to_string).collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || {
            let alpn = alpn.iter().map(String::as_str).collect::<Vec<_>>();
            let connector = native_tls::TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .request_alpns(&alpn)
                .build()
                .map_err(std::io::Error::other)?;
            let stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
            stream.set_read_timeout(Some(Duration::from_secs(10)))?;
            let mut stream = connector
                .connect(&server_name, stream)
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            std::io::Write::write_all(&mut stream, request.as_bytes())?;
            let mut response = String::new();
            std::io::Read::read_to_string(&mut stream, &mut response)?;
            Ok(response)
        })
        .await
        .unwrap()
    }

    /// Starts HTTP/2 over TLS (ALPN `h2`) with proksi on `port`, asking for `server_name`,
    /// returning the header of the first frame proksi sends back
    pub async fn h2_first_frame(&self, port: u16, server_name: &str) -> std::io::Result<[u8; 9]> {
        let server_name = server_name.to_string();
        tokio::task::spawn_blocking(move || {
            let connector = native_tls::TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .request_alpns(&["h2"])
                .build()
                .map_err(std::io::Error::other)?;
            let stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
            stream.set_read_timeout(Some(Duration::from_secs(10)))?;
            let mut stream = connector
                .connect(&server_name, stream)
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            // The connection preface, then an empty SETTINGS frame
            std::io::Write::write_all(&mut stream, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")?;
            std::io::Write::write_all(&mut stream, &[0, 0, 0, 4, 0, 0, 0, 0, 0])?;
            let mut frame = [0; 9];
            std::io::Read::read_exact(&mut stream, &mut frame)?;
            Ok(frame)
        })
        .await
        .unwrap()
    }

    /// Opens a websocket connection through proksi
    pub async fn websocket(&self, host: &str, path: impl AsRef<str>) -> Result<WebSocket, String> {
        let path = path.as_ref();
        let tcp = TcpStream::connect(("127.0.0.1", self.https_port))
            .await
            .map_err(|e| e.to_string())?;
        let tls = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| e.to_string())?;

        let url = format!("wss://{host}:{}{path}", self.https_port);
        let (ws, _) = tokio_tungstenite::client_async_tls_with_config(
            url,
            tcp,
            None,
            Some(Connector::NativeTls(tls)),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(ws)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.dir.path().join("cache")
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn logs(&self) -> String {
        let read =
            |name: &str| std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default();
        format!(
            "--- stdout\n{}\n--- stderr\n{}",
            read("stdout.log"),
            read("stderr.log")
        )
    }

    /// Access log line for the given path (and query)
    pub fn access_log(&self, path_and_query: &str) -> String {
        let (path, query) = path_and_query
            .split_once('?')
            .unwrap_or((path_and_query, ""));
        let stdout =
            std::fs::read_to_string(self.dir.path().join("stdout.log")).unwrap_or_default();
        stdout
            .lines()
            .find(|l| {
                l.contains("\"access_log\":true")
                    && l.contains(&format!("\"path\":\"{path}\""))
                    && l.contains(&format!("\"query\":\"{query}\""))
            })
            .unwrap_or("no access log")
            .to_string()
    }

    /// Access logs of TCP connections (their fields), waiting up to 2 seconds for `count` of them
    pub async fn tcp_access_logs(&self, count: usize) -> Vec<serde_json::Value> {
        let mut logs = vec![];
        for _ in 0..20 {
            let stdout =
                std::fs::read_to_string(self.dir.path().join("stdout.log")).unwrap_or_default();
            logs = stdout
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .map(|line| line.get("fields").cloned().unwrap_or(line))
                .filter(|fields| fields["access_log"] == true && fields["protocol"] == "tcp")
                .collect();
            if logs.len() >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        logs
    }

    /// Warning and error lines from proksi's logs (and anything on stderr)
    pub fn error_logs(&self) -> String {
        let stdout =
            std::fs::read_to_string(self.dir.path().join("stdout.log")).unwrap_or_default();
        let stderr =
            std::fs::read_to_string(self.dir.path().join("stderr.log")).unwrap_or_default();
        let mut lines = stdout
            .lines()
            .filter(|l| l.contains("\"level\":\"ERROR\"") || l.contains("\"level\":\"WARN\""))
            .take(50)
            .collect::<Vec<_>>()
            .join("\n");
        lines.push_str("\n--- stderr\n");
        lines.push_str(&stderr);
        lines
    }

    /// Asserts proksi is still running and didn't panic
    pub fn assert_healthy(&mut self) {
        assert!(self.is_alive(), "proksi is not running:\n{}", self.logs());

        let logs = self.logs();
        assert!(!logs.contains("panicked"), "proksi panicked:\n{logs}");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Sends a text message through the websocket and returns the echoed message
pub async fn websocket_echo(ws: &mut WebSocket, text: &str) -> Result<String, String> {
    ws.send(Message::text(text))
        .await
        .map_err(|e| e.to_string())?;

    let reply = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .map_err(|_| "timed out waiting for echo".to_string())?;

    match reply {
        Some(Ok(Message::Text(reply))) => Ok(reply.to_string()),
        other => Err(format!("unexpected websocket message: {other:?}")),
    }
}

/// HTTP/1.1 upstream that echoes the request it received as JSON
pub struct HttpUpstream {
    pub port: u16,
    hits: Arc<Mutex<HashMap<String, u32>>>,
}

impl HttpUpstream {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(Mutex::new(HashMap::new()));

        let state = hits.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(handle_http_connection(stream, state.clone()));
            }
        });

        Self { port, hits }
    }

    /// Number of requests the upstream received for the given path (and query)
    pub fn hits(&self, path: &str) -> u32 {
        self.hits
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .unwrap_or_default()
    }
}

/// Reads a chunked request body, returning its length (`None` if the connection closed)
async fn read_chunked_body(reader: &mut BufReader<TcpStream>) -> Option<usize> {
    let mut length = 0;
    loop {
        let mut size = String::new();
        if reader.read_line(&mut size).await.ok()? == 0 {
            return None;
        }
        let size = usize::from_str_radix(size.trim(), 16).ok()?;
        // The chunk and its trailing CRLF (the last chunk is only a CRLF)
        let mut chunk = vec![0; size + 2];
        reader.read_exact(&mut chunk).await.ok()?;
        if size == 0 {
            return Some(length);
        }
        length += size;
    }
}

async fn handle_http_connection(stream: TcpStream, hits: Arc<Mutex<HashMap<String, u32>>>) {
    let mut reader = BufReader::new(stream);

    // Keep-alive: serve requests until the connection is closed
    loop {
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).await.unwrap_or(0) == 0 {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

        let mut headers = serde_json::Map::new();
        let mut content_length = 0;
        let mut chunked = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_lowercase();
                let value = value.trim().to_string();
                if name == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                if name == "transfer-encoding" && value.contains("chunked") {
                    chunked = true;
                }
                headers.insert(name, value.into());
            }
        }
        // Counted once the headers arrive, so requests ended while sending their body count too
        let hit_count = {
            let mut hits = hits.lock().unwrap();
            let count = hits.entry(path.clone()).or_default();
            *count += 1;
            *count
        };

        let body_length = if chunked {
            let Some(length) = read_chunked_body(&mut reader).await else {
                return;
            };
            length
        } else {
            let mut body = vec![0; content_length];
            if reader.read_exact(&mut body).await.is_err() {
                return;
            }
            content_length
        };

        let body = serde_json::json!({
            "method": method,
            "path": path,
            "hits": hit_count,
            "headers": headers,
            "body_length": body_length,
        })
        .to_string();

        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: public, max-age=60\r\nContent-Length: {}\r\n",
            body.len()
        );
        if path.contains("utf8") {
            let _ = write!(
                response,
                "Content-Disposition: {UTF8_CONTENT_DISPOSITION}\r\n"
            );
        }
        response.push_str("\r\n");
        response.push_str(&body);

        if reader
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .is_err()
        {
            return;
        }
        if headers
            .get("connection")
            .is_some_and(|value| value.as_str() == Some("close"))
        {
            return;
        }
    }
}

/// Websocket upstream that echoes every message back
pub struct WebSocketUpstream {
    pub port: u16,
    /// How each connection ended, by request path
    events: Arc<Mutex<HashMap<String, String>>>,
}

impl WebSocketUpstream {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let events = Arc::new(Mutex::new(HashMap::new()));

        let state = events.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(handle_websocket_connection(stream, state.clone()));
            }
        });

        Self { port, events }
    }

    /// How the connection for the given path ended (if it was received at all)
    pub fn event(&self, path: &str) -> Option<String> {
        self.events.lock().unwrap().get(path).cloned()
    }
}

async fn handle_websocket_connection(
    stream: TcpStream,
    events: Arc<Mutex<HashMap<String, String>>>,
) {
    let path = Arc::new(Mutex::new(String::new()));
    let request_path = path.clone();
    let accept = tokio_tungstenite::accept_hdr_async(
        stream,
        move |req: &tokio_tungstenite::tungstenite::handshake::server::Request, res| {
            *request_path.lock().unwrap() = req.uri().to_string();
            Ok(res)
        },
    )
    .await;
    let path = path.lock().unwrap().clone();
    let record = |event: String| {
        events.lock().unwrap().insert(path.clone(), event);
    };

    let mut ws = match accept {
        Ok(ws) => ws,
        Err(err) => return record(format!("handshake failed: {err}")),
    };
    record("open".to_string());

    loop {
        match ws.next().await {
            Some(Ok(message)) if message.is_text() || message.is_binary() => {
                if let Err(err) = ws.send(message).await {
                    return record(format!("send failed: {err}"));
                }
                record("echoed".to_string());
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => return record(format!("read failed: {err}")),
            None => return record("closed by proksi".to_string()),
        }
    }
}

fn free_port() -> u16 {
    StdTcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Raises the soft limit of open files (e.g. macOS defaults to 256),
/// which is inherited by the proksi process
fn raise_open_files_limit() {
    use nix::sys::resource::{getrlimit, setrlimit, Resource};

    if let Ok((soft, hard)) = getrlimit(Resource::RLIMIT_NOFILE) {
        let target = hard.min(8192);
        if soft < target {
            let _ = setrlimit(Resource::RLIMIT_NOFILE, target, hard);
        }
    }
}

/// Writes a self-signed certificate (valid for `*.test`) to `cert.pem` and `key.pem`
fn write_self_signed_cert(dir: &Path) {
    let rsa = Rsa::generate(2048).unwrap();
    let key = PKey::from_rsa(rsa).unwrap();

    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "proksi.test").unwrap();
    let name = name.build();

    let mut serial = BigNum::new().unwrap();
    serial.rand(64, MsbOption::MAYBE_ZERO, false).unwrap();

    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_serial_number(&serial.to_asn1_integer().unwrap())
        .unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(&key).unwrap();
    cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    cert.set_not_after(&Asn1Time::days_from_now(2).unwrap())
        .unwrap();
    let san = SubjectAlternativeName::new()
        .dns("*.test")
        .build(&cert.x509v3_context(None, None))
        .unwrap();
    cert.append_extension(san).unwrap();
    cert.sign(&key, MessageDigest::sha256()).unwrap();
    let cert = cert.build();

    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.to_pem().unwrap()).unwrap();
    std::fs::write(&key_path, key.private_key_to_pem_pkcs8().unwrap()).unwrap();
}
