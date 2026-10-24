//! A TXT query, sent straight to a name server: enough to tell when a DNS-01 record can be
//! seen, without a resolver (and its cache) in between

use std::{io, net::SocketAddr, time::Duration};

use tokio::net::UdpSocket;

/// How long a name server has to answer
const TIMEOUT: Duration = Duration::from_secs(3);

/// TXT records, and the IN class
const TXT: u16 = 16;
const IN: u16 = 1;

/// The TXT records of `name`, as `server` answers them
pub async fn txt_records(server: SocketAddr, name: &str) -> io::Result<Vec<String>> {
    let id = query_id();
    let query = encode_query(id, name)?;

    let local: SocketAddr = if server.is_ipv4() {
        ([0, 0, 0, 0], 0).into()
    } else {
        ([0u16; 8], 0).into()
    };
    let socket = UdpSocket::bind(local).await?;
    socket.connect(server).await?;
    socket.send(&query).await?;

    let mut buffer = [0u8; 4096];
    let read = tokio::time::timeout(TIMEOUT, socket.recv(&mut buffer))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no answer"))??;
    decode_txt(id, buffer.get(..read).unwrap_or_default())
}

/// Not cryptographic: the answer comes back on the socket the query was sent from
fn query_id() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or_default();
    u16::try_from(nanos & 0xffff).unwrap_or_default()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// A query for the TXT records of `name`, without recursion (the server is authoritative)
fn encode_query(id: u16, name: &str) -> io::Result<Vec<u8>> {
    let mut query = Vec::with_capacity(name.len() + 18);
    query.extend_from_slice(&id.to_be_bytes());
    // Flags (a standard query, no recursion), 1 question, no other records
    query.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        let length = u8::try_from(label.len())
            .ok()
            .filter(|length| (1..64).contains(length))
            .ok_or_else(|| invalid("invalid name"))?;
        query.push(length);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&TXT.to_be_bytes());
    query.extend_from_slice(&IN.to_be_bytes());
    Ok(query)
}

/// The TXT records in the answer to query `id`
fn decode_txt(id: u16, message: &[u8]) -> io::Result<Vec<String>> {
    let mut reader = Reader { message, at: 0 };
    if reader.u16()? != id {
        return Err(invalid("answer to another query"));
    }
    let flags = reader.u16()?;
    // NXDOMAIN: no such name (yet), so no records
    match flags & 0x000f {
        0 => {}
        3 => return Ok(vec![]),
        code => return Err(invalid(&format!("name server error {code}"))),
    }
    let questions = reader.u16()?;
    let answers = reader.u16()?;
    reader.skip(4)?;

    for _ in 0..questions {
        reader.skip_name()?;
        reader.skip(4)?;
    }
    let mut records = vec![];
    for _ in 0..answers {
        reader.skip_name()?;
        let kind = reader.u16()?;
        reader.skip(6)?;
        let length = usize::from(reader.u16()?);
        let data = reader.take(length)?;
        if kind == TXT {
            records.push(character_strings(data)?);
        }
    }
    Ok(records)
}

/// A TXT record's strings (each prefixed by its length), joined
fn character_strings(mut data: &[u8]) -> io::Result<String> {
    let mut text = Vec::with_capacity(data.len());
    while let Some((&length, rest)) = data.split_first() {
        let length = usize::from(length);
        let chunk = rest
            .get(..length)
            .ok_or_else(|| invalid("truncated record"))?;
        text.extend_from_slice(chunk);
        data = rest.get(length..).unwrap_or_default();
    }
    String::from_utf8(text).map_err(|_| invalid("record is not text"))
}

struct Reader<'a> {
    message: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let bytes = self
            .message
            .get(self.at..self.at + count)
            .ok_or_else(|| invalid("truncated answer"))?;
        self.at += count;
        Ok(bytes)
    }

    fn skip(&mut self, count: usize) -> io::Result<()> {
        self.take(count).map(|_| ())
    }

    fn u16(&mut self) -> io::Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([
            bytes.first().copied().unwrap_or_default(),
            bytes.get(1).copied().unwrap_or_default(),
        ]))
    }

    /// Skips a name: labels, ending with an empty one or a pointer to an earlier name
    fn skip_name(&mut self) -> io::Result<()> {
        loop {
            let length = *self.take(1)?.first().unwrap_or(&0);
            match length {
                0 => return Ok(()),
                pointer if pointer & 0xc0 == 0xc0 => return self.skip(1),
                label => self.skip(usize::from(label))?,
            }
        }
    }
}

/// A name server answering every query with `records`
#[cfg(test)]
pub async fn fake_server(records: Vec<String>) -> SocketAddr {
    let server = UdpSocket::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|err| panic!("{err}"));
    let address = server.local_addr().unwrap_or_else(|err| panic!("{err}"));
    tokio::spawn(async move {
        let mut buffer = [0u8; 512];
        while let Ok((read, from)) = server.recv_from(&mut buffer).await {
            let strings = records
                .iter()
                .map(|record| vec![record.as_str()])
                .collect::<Vec<_>>();
            let strings = strings.iter().map(Vec::as_slice).collect::<Vec<_>>();
            let reply = tests::answer(buffer.get(..read).unwrap_or_default(), &strings);
            let _ = server.send_to(&reply, from).await;
        }
    });
    address
}

#[cfg(test)]
mod tests {
    use tokio::net::UdpSocket;

    use super::{decode_txt, encode_query, txt_records};

    /// An answer to `query` with `records`, its names pointing to the question's
    pub fn answer(query: &[u8], records: &[&[&str]]) -> Vec<u8> {
        let mut answer = query.to_vec();
        // An answer, from an authoritative server
        answer[2] = 0x84;
        answer[7] = u8::try_from(records.len()).unwrap();
        for strings in records {
            answer.extend_from_slice(&[0xc0, 12, 0, 16, 0, 1, 0, 0, 0, 60]);
            let data = strings
                .iter()
                .flat_map(|text| {
                    std::iter::once(u8::try_from(text.len()).unwrap()).chain(text.bytes())
                })
                .collect::<Vec<_>>();
            answer.extend_from_slice(&u16::try_from(data.len()).unwrap().to_be_bytes());
            answer.extend_from_slice(&data);
        }
        answer
    }

    #[test]
    fn test_txt_records_of_an_answer() {
        let query = encode_query(7, "_acme-challenge.example.test").unwrap();
        let message = answer(&query, &[&["proof"], &["split ", "in two"]]);

        assert_eq!(
            decode_txt(7, &message).unwrap(),
            vec!["proof".to_string(), "split in two".to_string()]
        );
        // Another query's answer, and a truncated one
        assert!(decode_txt(8, &message).is_err());
        assert!(decode_txt(7, &message[..message.len() - 3]).is_err());
    }

    #[test]
    fn test_no_such_name_has_no_records() {
        let mut message = encode_query(7, "_acme-challenge.example.test").unwrap();
        message[2] = 0x84;
        message[3] = 3;
        assert_eq!(decode_txt(7, &message).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn test_invalid_names() {
        assert!(encode_query(1, "a..test").is_err());
        assert!(encode_query(1, &format!("{}.test", "a".repeat(64))).is_err());
        assert!(encode_query(1, "example.test.").is_ok());
    }

    #[tokio::test]
    async fn test_asks_the_server() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buffer = [0u8; 512];
            let (read, from) = server.recv_from(&mut buffer).await.unwrap();
            let reply = answer(&buffer[..read], &[&["token"]]);
            server.send_to(&reply, from).await.unwrap();
        });

        let records = txt_records(address, "_acme-challenge.example.test")
            .await
            .unwrap();
        assert_eq!(records, vec!["token".to_string()]);
    }
}
