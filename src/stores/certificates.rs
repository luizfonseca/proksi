use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Certificate {
    pub key: openssl::pkey::PKey<openssl::pkey::Private>,
    pub certificate: openssl::x509::X509,
}

pub type CertificateStore = HashMap<String, Certificate>;
