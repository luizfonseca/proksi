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
pub(crate) struct SerializableCertificate {
    key: String,
    leaf: String,
    /// The intermediates, concatenated (stored before they were all kept: only the first)
    chain: Option<String>,
}

impl Certificate {
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
pub(crate) mod tests {
    use openssl::{
        asn1::Asn1Time,
        ec::{EcGroup, EcKey},
        hash::MessageDigest,
        nid::Nid,
        pkey::{PKey, Private},
        x509::{X509Name, X509},
    };

    use super::Certificate;

    /// A certificate for `name`, signed by `issuer` (self-signed without one)
    pub(crate) fn signed(
        name: &str,
        issuer: Option<&(X509, PKey<Private>)>,
    ) -> (X509, PKey<Private>) {
        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
        let mut subject = X509Name::builder().unwrap();
        subject.append_entry_by_text("CN", name).unwrap();
        let subject = subject.build();
        let mut builder = X509::builder().unwrap();
        builder.set_subject_name(&subject).unwrap();
        builder
            .set_issuer_name(issuer.map_or(&subject, |(issuer, _)| issuer.subject_name()))
            .unwrap();
        builder.set_pubkey(&key).unwrap();
        builder
            .set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        builder
            .set_not_after(&Asn1Time::days_from_now(30).unwrap())
            .unwrap();
        builder
            .sign(issuer.map_or(&key, |(_, key)| key), MessageDigest::sha256())
            .unwrap();
        (builder.build(), key)
    }

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
