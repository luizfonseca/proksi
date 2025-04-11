use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};

#[derive(Debug, Clone)]
pub struct Certificate {
    pub key: PKey<Private>,
    pub leaf: X509,
    pub chain: Option<X509>,
}
