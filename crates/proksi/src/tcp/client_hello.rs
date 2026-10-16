//! The server name (SNI) of a TLS `ClientHello`, read before the handshake so a connection can be
//! forwarded untouched (TLS passthrough) or handshaken by proksi

/// What the first TLS record of a connection says
#[derive(Debug, PartialEq)]
pub enum Hello {
    /// Not a TLS handshake
    NotTls,
    /// A `ClientHello`, with the server name the client asked for, if any
    ClientHello(Option<String>),
}

/// Length of a TLS record's header
pub const RECORD_HEADER: usize = 5;
/// Largest record allowed (2^14 bytes plus expansion)
pub const MAX_RECORD: usize = 16_384 + 2048;

/// The length of the record whose header is `header` (5 bytes), if it's a TLS handshake
pub fn record_length(header: &[u8]) -> Option<usize> {
    let mut reader = Reader(header);
    // A handshake record, of TLS 1.x
    if reader.u8()? != 0x16 || reader.u8()? != 0x03 {
        return None;
    }
    reader.u8()?;
    let length = usize::from(reader.u16()?);
    (length <= MAX_RECORD).then_some(length)
}

/// Reads a whole record (header and body)
pub fn parse(record: &[u8]) -> Hello {
    client_hello(record).unwrap_or(Hello::NotTls)
}

/// The `ClientHello` in `record`, or none if it isn't one (or is truncated)
fn client_hello(record: &[u8]) -> Option<Hello> {
    let length = record_length(record.get(..RECORD_HEADER)?)?;
    let mut reader = Reader(record.get(RECORD_HEADER..RECORD_HEADER + length)?);

    // Handshake message: `ClientHello` (1), and its length
    if reader.u8()? != 1 {
        return None;
    }
    reader.skip(3)?;
    // Version, random, session id, cipher suites, compression methods
    reader.skip(2 + 32)?;
    let session_id = usize::from(reader.u8()?);
    reader.skip(session_id)?;
    let cipher_suites = usize::from(reader.u16()?);
    reader.skip(cipher_suites)?;
    let compression = usize::from(reader.u8()?);
    reader.skip(compression)?;

    // No extensions: no server name
    let Some(extensions_length) = reader.u16() else {
        return Some(Hello::ClientHello(None));
    };
    let mut extensions = Reader(reader.take(usize::from(extensions_length))?);
    while let Some(kind) = extensions.u16() {
        let length = usize::from(extensions.u16()?);
        let data = extensions.take(length)?;
        // server_name: a list of names, of which host_name (0) is the only kind
        if kind == 0 {
            let mut list = Reader(data);
            let list_length = usize::from(list.u16()?);
            let mut names = Reader(list.take(list_length)?);
            while let Some(name_type) = names.u8() {
                let name_length = usize::from(names.u16()?);
                let name = names.take(name_length)?;
                if name_type == 0 {
                    let name = std::str::from_utf8(name).ok()?;
                    return Some(Hello::ClientHello(Some(name.to_ascii_lowercase())));
                }
            }
        }
    }
    Some(Hello::ClientHello(None))
}

/// Reads big-endian numbers and slices, never past the end
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.0.split_at_checked(length)?;
        self.0 = rest;
        Some(taken)
    }

    fn skip(&mut self, length: usize) -> Option<()> {
        self.take(length).map(|_| ())
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u16(&mut self) -> Option<u16> {
        let bytes = self.take(2)?;
        Some(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use openssl::ssl::{SslConnector, SslMethod};

    use super::{parse, record_length, Hello, RECORD_HEADER};

    /// Records what a client writes, and never answers
    #[derive(Default)]
    struct Recorder(Vec<u8>);

    impl Read for Recorder {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::WouldBlock.into())
        }
    }

    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A `ClientHello` written by OpenSSL, asking for `server_name`
    fn client_hello(server_name: Option<&str>) -> Vec<u8> {
        let connector = SslConnector::builder(SslMethod::tls()).unwrap().build();
        let mut config = connector.configure().unwrap();
        config.set_use_server_name_indication(server_name.is_some());
        config.set_verify_hostname(false);
        let mut recorder = Recorder::default();
        let _ = config.connect(server_name.unwrap_or("ignored"), &mut recorder);
        recorder.0
    }

    #[test]
    fn test_server_name() {
        let hello = client_hello(Some("Pass.Test"));
        let length = record_length(&hello[..RECORD_HEADER]).unwrap();
        assert_eq!(hello.len(), RECORD_HEADER + length);

        // Names are case-insensitive
        assert_eq!(parse(&hello), Hello::ClientHello(Some("pass.test".to_string())));
    }

    #[test]
    fn test_without_a_server_name() {
        assert_eq!(parse(&client_hello(None)), Hello::ClientHello(None));
    }

    #[test]
    fn test_not_tls() {
        assert_eq!(record_length(b"GET / HTTP/1.1"), None);
        assert_eq!(parse(b"GET / HTTP/1.1\r\n\r\n"), Hello::NotTls);
        // Too large for a record
        assert_eq!(record_length(&[0x16, 0x03, 0x01, 0xff, 0xff]), None);
    }

    #[test]
    fn test_truncated() {
        let hello = client_hello(Some("pass.test"));
        for length in [0, 3, RECORD_HEADER, 40, hello.len() - 1] {
            assert_eq!(parse(&hello[..length]), Hello::NotTls, "{length}");
        }
    }
}
