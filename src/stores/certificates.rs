use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;

#[derive(Debug)]
pub struct Certificate {
    pub key: Bytes,
    pub certificate: Bytes,
}

pub type CertificateStore = Arc<DashMap<String, Certificate>>;
