//! Certificates for tests, here and in Proksi (`test-util` feature)
#![allow(clippy::unwrap_used)]

use openssl::{
    asn1::Asn1Time,
    ec::{EcGroup, EcKey},
    hash::MessageDigest,
    nid::Nid,
    pkey::{PKey, Private},
    rsa::Rsa,
    x509::{X509Name, X509},
};

use crate::certificates::Certificate;

/// A self-signed certificate for `domain`, valid for `days`
///
/// # Panics
///
/// When OpenSSL fails to create it
#[must_use]
pub fn certificate(domain: &str, days: u32) -> Certificate {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509Name::builder().unwrap();
    name.append_entry_by_text("CN", domain).unwrap();
    let name = name.build();

    let mut leaf = X509::builder().unwrap();
    leaf.set_version(2).unwrap();
    leaf.set_subject_name(&name).unwrap();
    leaf.set_issuer_name(&name).unwrap();
    leaf.set_pubkey(&key).unwrap();
    leaf.set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    leaf.set_not_after(&Asn1Time::days_from_now(days).unwrap())
        .unwrap();
    leaf.sign(&key, MessageDigest::sha256()).unwrap();

    Certificate {
        key,
        leaf: leaf.build(),
        chain: vec![],
    }
}

/// A certificate for `name`, signed by `issuer` (self-signed without one)
///
/// # Panics
///
/// When OpenSSL fails to create it
#[must_use]
pub fn signed(
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
