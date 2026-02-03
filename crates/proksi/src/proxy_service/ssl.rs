// SSL handling utilities for individual proxies

use crate::config::ProtoVersion;

impl From<&ProtoVersion> for pingora::tls::ssl::SslVersion {
    fn from(v: &ProtoVersion) -> Self {
        match v {
            ProtoVersion::V1_1 => pingora::tls::ssl::SslVersion::TLS1_1,
            ProtoVersion::V1_2 => pingora::tls::ssl::SslVersion::TLS1_2,
            ProtoVersion::V1_3 => pingora::tls::ssl::SslVersion::TLS1_3,
        }
    }
}