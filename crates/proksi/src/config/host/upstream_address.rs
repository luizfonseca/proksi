/// Parses an upstream label: `host:port` or `[ipv6]:port`
pub fn parse_upstream_address(label: &str) -> Result<(String, u16), String> {
    let Some((ip, port)) = label.rsplit_once(':') else {
        return Err(format!("upstream \"{label}\" needs a port (host:port)"));
    };
    let bracketed = ip.starts_with('[') && ip.ends_with(']');
    if ip.is_empty() || (ip.contains(':') && !bracketed) {
        return Err(format!(
            "upstream \"{label}\" is not a valid address (host:port or [ipv6]:port)"
        ));
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("upstream \"{label}\" has an invalid port"))?;

    Ok((ip.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::parse_upstream_address;

    #[test]
    fn test_parse_upstream_address() {
        assert_eq!(
            parse_upstream_address("directory:8080"),
            Ok(("directory".to_string(), 8080))
        );
        assert_eq!(
            parse_upstream_address("10.0.0.1:3000"),
            Ok(("10.0.0.1".to_string(), 3000))
        );
        assert_eq!(
            parse_upstream_address("[::1]:3000"),
            Ok(("[::1]".to_string(), 3000))
        );
    }

    #[test]
    fn test_parse_upstream_address_without_port() {
        assert_eq!(
            parse_upstream_address("10.0.0.1"),
            Err("upstream \"10.0.0.1\" needs a port (host:port)".to_string())
        );
    }

    #[test]
    fn test_parse_invalid_upstream_address() {
        let invalid = "is not a valid address (host:port or [ipv6]:port)";
        assert_eq!(
            parse_upstream_address("::1:3000"),
            Err(format!("upstream \"::1:3000\" {invalid}"))
        );
        assert_eq!(
            parse_upstream_address(":3000"),
            Err(format!("upstream \":3000\" {invalid}"))
        );
    }

    #[test]
    fn test_parse_upstream_address_with_invalid_port() {
        assert_eq!(
            parse_upstream_address("a.test:0"),
            Err("upstream \"a.test:0\" has an invalid port".to_string())
        );
        assert_eq!(
            parse_upstream_address("a.test:http"),
            Err("upstream \"a.test:http\" has an invalid port".to_string())
        );
    }
}
