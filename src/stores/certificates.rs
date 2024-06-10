use std::sync::Arc;

use dashmap::DashMap;

#[derive(Debug)]
pub struct Certificate {
    pub key: openssl::pkey::PKey<openssl::pkey::Private>,
    pub certificate: openssl::x509::X509,
}

pub type CertificateStore = Arc<DashMap<String, Certificate>>;
