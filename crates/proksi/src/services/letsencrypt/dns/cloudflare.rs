//! TXT records through Cloudflare's API, with a token allowed to edit DNS records
//! (`Zone:DNS:Edit`)

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{DnsProvider, DnsRecord};

const API: &str = "https://api.cloudflare.com/client/v4";

/// How long records live: short, as they're removed right after the challenge
const TTL_SECS: u32 = 60;

pub struct Cloudflare {
    client: reqwest::Client,
    token: String,
    api: String,
}

/// What every answer of the API looks like
#[derive(Deserialize)]
struct Answer<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<ApiError>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Deserialize)]
struct Zone {
    id: String,
    #[serde(default)]
    name_servers: Vec<String>,
}

#[derive(Deserialize)]
struct Record {
    id: String,
}

impl Cloudflare {
    pub fn new(token: String) -> Self {
        Self::with_api(token, API.to_string())
    }

    fn with_api(token: String, api: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            token,
            api,
        }
    }

    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<Option<T>, String> {
        let response = request
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|err| format!("Cloudflare can't be reached: {err}"))?;
        let status = response.status();
        let answer = response
            .json::<Answer<T>>()
            .await
            .map_err(|err| format!("unexpected answer from Cloudflare ({status}): {err}"))?;
        if !answer.success {
            let messages = answer
                .errors
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("Cloudflare refused ({status}): {messages}"));
        }
        Ok(answer.result)
    }

    /// The zone `name` is in: the longest of its parents Cloudflare has a zone for
    /// (`_acme-challenge.a.example.com` → `a.example.com`, then `example.com`)
    async fn zone(&self, name: &str) -> Result<Zone, String> {
        let mut candidate = name;
        while let Some((_, parent)) = candidate.split_once('.') {
            if !parent.contains('.') {
                break;
            }
            let request = self
                .client
                .get(format!("{}/zones", self.api))
                .query(&[("name", parent)]);
            let zones: Vec<Zone> = self.call(request).await?.unwrap_or_default();
            if let Some(zone) = zones.into_iter().next() {
                return Ok(zone);
            }
            candidate = parent;
        }
        Err(format!("no Cloudflare zone holds {name}"))
    }
}

#[async_trait]
impl DnsProvider for Cloudflare {
    async fn set_txt(&self, name: &str, value: &str) -> Result<DnsRecord, String> {
        let zone = self.zone(name).await?;
        let request = self
            .client
            .post(format!("{}/zones/{}/dns_records", self.api, zone.id))
            .json(&json!({ "type": "TXT", "name": name, "content": value, "ttl": TTL_SECS }));
        let record: Record = self
            .call(request)
            .await?
            .ok_or("Cloudflare didn't return the record")?;
        Ok(DnsRecord {
            name: name.to_string(),
            value: value.to_string(),
            zone_id: zone.id,
            id: record.id,
            name_servers: zone.name_servers,
        })
    }

    async fn remove_txt(&self, record: &DnsRecord) -> Result<(), String> {
        let request = self.client.delete(format!(
            "{}/zones/{}/dns_records/{}",
            self.api, record.zone_id, record.id
        ));
        self.call::<serde_json::Value>(request).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{Cloudflare, DnsProvider, DnsRecord};

    /// A request the fake API received: method, path and query, body
    type Received = Arc<Mutex<Vec<(String, String, Value)>>>;

    /// A fake API answering `respond(method, path)`, one request per connection
    async fn api(respond: fn(&str, &str) -> (u16, Value)) -> (String, Received) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Received::default();
        let log = received.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                answer(stream, respond, &log).await;
            }
        });
        (format!("http://{address}"), received)
    }

    /// Answers one request of the fake API
    async fn answer(
        mut stream: tokio::net::TcpStream,
        respond: fn(&str, &str) -> (u16, Value),
        log: &Received,
    ) {
        let mut request = vec![];
        let mut buffer = [0u8; 4096];
        // Headers, then the body they announce
        let (head, body) = loop {
            let read = stream.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..read]);
            if let Some(whole) = whole_request(&request) {
                break whole;
            }
        };
        let mut line = head.lines().next().unwrap().split(' ');
        let (method, path) = (line.next().unwrap(), line.next().unwrap());
        assert!(head.contains("authorization: Bearer token"), "{head}");
        let body = serde_json::from_str(&body).unwrap_or(Value::Null);
        log.lock()
            .unwrap()
            .push((method.to_string(), path.to_string(), body));

        let (status, answer) = respond(method, path);
        let answer = answer.to_string();
        let response = format!(
            "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
            answer.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    }

    /// The headers and body of a request, once the body they announce arrived
    fn whole_request(request: &[u8]) -> Option<(String, String)> {
        let text = String::from_utf8_lossy(request).to_string();
        let (head, body) = text.split_once("\r\n\r\n")?;
        let length = head
            .lines()
            .find_map(|line| {
                line.to_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|value| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        (body.len() >= length).then(|| (head.to_string(), body.to_string()))
    }

    fn zones(path: &str) -> (u16, Value) {
        // Only `example.test` is a zone
        let result = if path == "/zones?name=example.test" {
            json!([{ "id": "zone-1", "name": "example.test", "name_servers": ["ns1.cf.test", "ns2.cf.test"] }])
        } else {
            json!([])
        };
        (
            200,
            json!({ "success": true, "errors": [], "result": result }),
        )
    }

    #[tokio::test]
    async fn test_sets_and_removes_a_record_in_the_zone() {
        let (url, received) = api(|method, path| match method {
            "GET" => zones(path),
            "POST" => (
                200,
                json!({ "success": true, "result": { "id": "record-1" } }),
            ),
            _ => (
                200,
                json!({ "success": true, "result": { "id": "record-1" } }),
            ),
        })
        .await;
        let cloudflare = Cloudflare::with_api("token".to_string(), url);

        let record = cloudflare
            .set_txt("_acme-challenge.a.example.test", "proof")
            .await
            .unwrap();
        assert_eq!(
            record,
            DnsRecord {
                name: "_acme-challenge.a.example.test".to_string(),
                value: "proof".to_string(),
                zone_id: "zone-1".to_string(),
                id: "record-1".to_string(),
                name_servers: vec!["ns1.cf.test".to_string(), "ns2.cf.test".to_string()],
            }
        );
        cloudflare.remove_txt(&record).await.unwrap();

        let received = received.lock().unwrap().clone();
        let requests = received
            .iter()
            .map(|(method, path, _)| format!("{method} {path}"))
            .collect::<Vec<_>>();
        assert_eq!(
            requests,
            vec![
                // The longest parent first, down to the zone
                "GET /zones?name=a.example.test",
                "GET /zones?name=example.test",
                "POST /zones/zone-1/dns_records",
                "DELETE /zones/zone-1/dns_records/record-1",
            ]
        );
        assert_eq!(
            received[2].2,
            json!({ "type": "TXT", "name": "_acme-challenge.a.example.test", "content": "proof", "ttl": 60 })
        );
    }

    #[tokio::test]
    async fn test_names_outside_every_zone() {
        let (url, _) = api(|_, path| zones(path)).await;
        let cloudflare = Cloudflare::with_api("token".to_string(), url);

        assert_eq!(
            cloudflare
                .set_txt("_acme-challenge.other.test", "proof")
                .await,
            Err("no Cloudflare zone holds _acme-challenge.other.test".to_string())
        );
    }

    #[tokio::test]
    async fn test_errors_of_the_api() {
        let (url, _) = api(|method, path| match method {
            "GET" => zones(path),
            _ => (
                403,
                json!({ "success": false, "errors": [{ "code": 10000, "message": "Authentication error" }], "result": null }),
            ),
        })
        .await;
        let cloudflare = Cloudflare::with_api("token".to_string(), url);

        assert_eq!(
            cloudflare
                .set_txt("_acme-challenge.example.test", "proof")
                .await,
            Err("Cloudflare refused (403 Forbidden): Authentication error".to_string())
        );
    }
}
