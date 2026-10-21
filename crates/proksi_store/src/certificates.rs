use openssl::{
    base64,
    pkey::{PKey, Private},
    x509::X509,
};
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Clone)]
pub struct Certificate {
    pub key: PKey<Private>,
    pub leaf: X509,
    /// The intermediates, in the order they're sent after the leaf
    pub chain: Vec<X509>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerializableCertificate {
    key: String,
    leaf: String,
    /// The intermediates, concatenated (stored before they were all kept: only the first)
    chain: Option<String>,
}

impl Certificate {
    /// The certificate as stored: its key and certificates in PEM, base64-encoded
    ///
    /// # Errors
    ///
    /// When the key or a certificate can't be encoded
    pub fn to_serializable(&self) -> Result<SerializableCertificate, Box<dyn Error>> {
        Ok(SerializableCertificate {
            key: base64::encode_block(&self.key.private_key_to_pem_pkcs8()?),
            leaf: base64::encode_block(&self.leaf.to_pem()?),
            chain: if self.chain.is_empty() {
                None
            } else {
                let pems = self
                    .chain
                    .iter()
                    .map(|c| c.to_pem())
                    .collect::<Result<Vec<_>, _>>()?;
                Some(base64::encode_block(&pems.concat()))
            },
        })
    }

    /// The certificate as stored, read back
    ///
    /// # Errors
    ///
    /// When the key or a certificate is invalid
    pub fn from_serializable(cert: SerializableCertificate) -> Result<Self, Box<dyn Error>> {
        let key_data = base64::decode_block(&cert.key)?;
        let leaf_data = base64::decode_block(&cert.leaf)?;

        let key = PKey::private_key_from_pem(&key_data)?;
        let leaf = X509::from_pem(&leaf_data)?;
        let chain = match cert.chain {
            Some(chain_b64) => X509::stack_from_pem(&base64::decode_block(&chain_b64)?)?,
            None => vec![],
        };

        Ok(Certificate { key, leaf, chain })
    }
}

#[cfg(test)]
mod tests {
    use openssl::x509::X509;

    use super::Certificate;
    use crate::test_util::signed;

    fn pems(certificates: &[X509]) -> Vec<Vec<u8>> {
        certificates.iter().map(|c| c.to_pem().unwrap()).collect()
    }

    #[test]
    fn test_every_intermediate_is_stored() {
        let first = signed("intermediate 1", None);
        let second = signed("intermediate 2", Some(&first));
        let (leaf, key) = signed("a.test", Some(&second));
        let certificate = Certificate {
            key,
            leaf,
            chain: vec![second.0.clone(), first.0.clone()],
        };

        let json = serde_json::to_string(&certificate.to_serializable().unwrap()).unwrap();
        let stored = Certificate::from_serializable(serde_json::from_str(&json).unwrap()).unwrap();

        assert_eq!(pems(&stored.chain), pems(&[second.0, first.0]));
        assert_eq!(
            stored.leaf.to_pem().unwrap(),
            certificate.leaf.to_pem().unwrap()
        );
    }

    #[test]
    fn test_certificates_without_chain_are_stored() {
        let (leaf, key) = signed("a.test", None);
        let certificate = Certificate {
            key,
            leaf,
            chain: vec![],
        };

        let serializable = certificate.to_serializable().unwrap();
        assert!(serializable.chain.is_none());
        assert!(Certificate::from_serializable(serializable)
            .unwrap()
            .chain
            .is_empty());
    }
}
