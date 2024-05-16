use std::collections::HashMap;

pub struct Certificate {
    pub key: String,
    pub certificate: String,
}

pub struct CertificatesStore {
    /// holds host + certificate + key
    certificates: HashMap<String, Certificate>,
}

impl CertificatesStore {
    pub fn new() -> Self {
        CertificatesStore {
            certificates: HashMap::new(),
        }
    }

    pub fn add_certificate(&mut self, host: String, certificate: String, key: String) {
        self.certificates
            .insert(host, Certificate { key, certificate });
    }

    pub fn get_certificate(&self, host: &str) -> Option<&Certificate> {
        self.certificates.get(host)
    }
}
