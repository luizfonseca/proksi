use openssl::{
    pkey::{PKey, Private},
    x509::X509,
    base64,
};
use serde::{Serialize, Deserialize};
use std::error::Error;

#[derive(Debug, Clone)]
pub struct Certificate {
    pub key: PKey<Private>,
    pub leaf: X509,
    pub chain: Option<X509>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SerializableCertificate {
    key: String,
    leaf: String,
    chain: Option<String>,
}

impl Certificate {
    pub fn to_serializable(&self) -> Result<SerializableCertificate, Box<dyn Error>> {
        Ok(SerializableCertificate {
            key: base64::encode_block(&self.key.private_key_to_pem_pkcs8()?),
            leaf: base64::encode_block(&self.leaf.to_pem()?),
            chain: self.chain.as_ref().map(|c| base64::encode_block(&c.to_pem().unwrap_or_default())),
        })
    }

    pub fn from_serializable(cert: SerializableCertificate) -> Result<Self, Box<dyn Error>> {
        let key_data = base64::decode_block(&cert.key)?;
        let leaf_data = base64::decode_block(&cert.leaf)?;
        
        let key = PKey::private_key_from_pem(&key_data)?;
        let leaf = X509::from_pem(&leaf_data)?;
        let chain = if let Some(chain_b64) = cert.chain {
            let chain_data = base64::decode_block(&chain_b64)?;
            Some(X509::from_pem(&chain_data)?)
        } else {
            None
        };

        Ok(Certificate { key, leaf, chain })
    }


}
