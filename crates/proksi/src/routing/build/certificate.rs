use anyhow::anyhow;
use openssl::{pkey::PKey, x509::X509};

use crate::{
    config::{host::Host, RouteSslPath},
    stores::{self, certificates::Certificate},
};

/// Loads the certificate of a host that has one in files (`tls.certificate`) into the store
pub async fn load_host_certificate(host: &Host) -> Result<(), anyhow::Error> {
    match &host.tls.certificate {
        Some(files) => load_certificate(&host.name, files).await,
        None => Ok(()),
    }
}

/// Loads a certificate from files into the store, under the lowercase `name`
pub(super) async fn load_certificate(
    name: &str,
    files: &RouteSslPath,
) -> Result<(), anyhow::Error> {
    let key = std::fs::read(&files.key)
        .map_err(anyhow::Error::from)
        .and_then(|key| Ok(PKey::private_key_from_pem(&key)?))
        .map_err(|err| anyhow!("failed to load private key from {:?}: {err}", files.key))?;
    // The leaf, followed by its chain
    let mut certificates = std::fs::read(&files.pem)
        .map_err(anyhow::Error::from)
        .and_then(|pem| Ok(X509::stack_from_pem(&pem)?))
        .map_err(|err| anyhow!("failed to load certificate from {:?}: {err}", files.pem))?
        .into_iter();
    let leaf = certificates
        .next()
        .ok_or_else(|| anyhow!("no certificate in {:?}", files.pem))?;

    stores::global::get_store()
        .set_certificate(
            &name.to_lowercase(),
            Certificate {
                key,
                leaf,
                chain: certificates.collect(),
            },
        )
        .await
        .map_err(|err| anyhow!("failed to store certificate for {name}: {err}"))
}

#[cfg(test)]
mod tests {
    use crate::config::{host::Host, RouteSslPath};

    #[tokio::test]
    async fn test_load_host_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let key =
            openssl::pkey::PKey::from_rsa(openssl::rsa::Rsa::generate(2048).unwrap()).unwrap();
        let mut cert = openssl::x509::X509::builder().unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&openssl::asn1::Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&openssl::asn1::Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        cert.sign(&key, openssl::hash::MessageDigest::sha256())
            .unwrap();
        let cert = cert.build();
        std::fs::write(
            dir.path().join("key.pem"),
            key.private_key_to_pem_pkcs8().unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("cert.pem"), cert.to_pem().unwrap()).unwrap();

        let mut host = crate::routing::test_utils::host("Host-Certificate.Test", "10.0.0.1");
        host.tls.certificate = Some(RouteSslPath {
            key: dir.path().join("key.pem"),
            pem: dir.path().join("cert.pem"),
        });

        super::load_host_certificate(&host).await.unwrap();

        let stored = crate::stores::global::get_store()
            .get_certificate("host-certificate.test")
            .await
            .unwrap();
        assert_eq!(stored.leaf.to_pem().unwrap(), cert.to_pem().unwrap());
    }

    #[tokio::test]
    async fn test_host_without_certificate_files() {
        let host: Host = crate::routing::test_utils::host("no-certificate.test", "10.0.0.1");
        assert!(super::load_host_certificate(&host).await.is_ok());
    }
}
