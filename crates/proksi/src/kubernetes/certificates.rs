//! Certificates of TLS `Secret`s, for the hosts listed in `Ingress`es' `tls` (ADR 0005)

use openssl::{pkey::PKey, x509::X509};

use crate::stores::certificates::Certificate;

/// A certificate from a `Secret`: `tls.crt` (the certificate, then its chain) and `tls.key`
pub fn parse(certificate: &[u8], key: &[u8]) -> Result<Certificate, String> {
    let mut certificates = X509::stack_from_pem(certificate)
        .map_err(|err| format!("invalid tls.crt: {err}"))?
        .into_iter();
    let leaf = certificates
        .next()
        .ok_or_else(|| "tls.crt has no certificate".to_string())?;
    let key = PKey::private_key_from_pem(key).map_err(|err| format!("invalid tls.key: {err}"))?;
    let matches = leaf.public_key().is_ok_and(|public| public.public_eq(&key));
    if !matches {
        return Err("tls.key doesn't match the certificate in tls.crt".to_string());
    }
    Ok(Certificate {
        key,
        leaf,
        chain: certificates.next(),
    })
}

#[cfg(test)]
mod tests {
    use openssl::{
        asn1::Asn1Time,
        hash::MessageDigest,
        pkey::{PKey, Private},
        rsa::Rsa,
        x509::{X509Name, X509},
    };

    fn certificate(name: &str, issuer: Option<(&X509, &PKey<Private>)>) -> (X509, PKey<Private>) {
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
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

    #[test]
    fn test_certificate_and_chain() {
        let (ca, ca_key) = certificate("intermediate", None);
        let (leaf, key) = certificate("a.test", Some((&ca, &ca_key)));
        let bundle = [leaf.to_pem().unwrap(), ca.to_pem().unwrap()].concat();

        let parsed = super::parse(&bundle, &key.private_key_to_pem_pkcs8().unwrap()).unwrap();

        assert_eq!(parsed.leaf.to_pem().unwrap(), leaf.to_pem().unwrap());
        assert_eq!(
            parsed.chain.unwrap().to_pem().unwrap(),
            ca.to_pem().unwrap()
        );
        assert!(parsed.key.public_eq(&key));
    }

    #[test]
    fn test_invalid_secrets() {
        let (leaf, key) = certificate("a.test", None);
        let (_, other_key) = certificate("b.test", None);
        let key = key.private_key_to_pem_pkcs8().unwrap();

        assert_eq!(
            super::parse(b"not a certificate", &key).err().as_deref(),
            Some("tls.crt has no certificate")
        );
        assert!(super::parse(&leaf.to_pem().unwrap(), b"not a key")
            .err()
            .unwrap()
            .starts_with("invalid tls.key"));
        assert_eq!(
            super::parse(
                &leaf.to_pem().unwrap(),
                &other_key.private_key_to_pem_pkcs8().unwrap()
            )
            .err()
            .as_deref(),
            Some("tls.key doesn't match the certificate in tls.crt")
        );
    }
}
