use std::{borrow::Cow, sync::Arc};

use dashmap::DashMap;

pub struct Certificate {
    pub key: String,
    pub certificate: String,
}

impl Certificate {
    pub fn new(key: String, certificate: String) -> Self {
        Certificate { key, certificate }
    }
}

pub type CertificateStore = Arc<DashMap<Cow<'static, str>, Certificate>>;
