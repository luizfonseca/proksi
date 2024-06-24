use std::collections::HashMap;

use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};

#[derive(Debug, Clone)]
pub struct Certificate {
    pub key: PKey<Private>,
    #[allow(clippy::struct_field_names)]
    pub leaf: X509,
    pub chain: Option<X509>,
}

pub type CertificateStore = HashMap<String, Certificate>;
