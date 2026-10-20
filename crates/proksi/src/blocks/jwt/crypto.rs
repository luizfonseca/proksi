//! Signatures checked with `ring`: RSA and ECDSA take ~10 µs there, against ~90 µs for RSA with
//! the pure Rust `rsa` crate. Signing (only tests sign) and HMAC stay with `rust_crypto`.

use jsonwebtoken::{
    crypto::{rust_crypto, CryptoProvider, JwtVerifier, KeyUtils},
    errors::{Error, ErrorKind},
    signature::{self, Verifier},
    Algorithm, DecodingKey, DecodingKeyKind,
};
use ring::signature::{self as ring_signature, RsaPublicKeyComponents, UnparsedPublicKey};

static PROVIDER: CryptoProvider = CryptoProvider {
    signer_factory: rust_crypto::DEFAULT_PROVIDER.signer_factory,
    verifier_factory: new_verifier,
    key_utils: KeyUtils {
        rsa_pub_components_from_private_key: rust_crypto::DEFAULT_PROVIDER
            .key_utils
            .rsa_pub_components_from_private_key,
        rsa_pub_components_from_public_key: rust_crypto::DEFAULT_PROVIDER
            .key_utils
            .rsa_pub_components_from_public_key,
        ec_pub_components_from_private_key: rust_crypto::DEFAULT_PROVIDER
            .key_utils
            .ec_pub_components_from_private_key,
        ed_pub_components_from_private_key: rust_crypto::DEFAULT_PROVIDER
            .key_utils
            .ed_pub_components_from_private_key,
        compute_digest: rust_crypto::DEFAULT_PROVIDER.key_utils.compute_digest,
    },
};

/// Makes tokens checked here, for the whole process: before any token is
pub fn install() {
    // Already installed
    let _ = PROVIDER.install_default();
}

/// The parameters of `algorithm`, unless it isn't checked here (HMAC)
fn parameters(algorithm: Algorithm) -> Option<&'static dyn ring_signature::VerificationAlgorithm> {
    Some(match algorithm {
        Algorithm::RS256 => &ring_signature::RSA_PKCS1_2048_8192_SHA256,
        Algorithm::RS384 => &ring_signature::RSA_PKCS1_2048_8192_SHA384,
        Algorithm::RS512 => &ring_signature::RSA_PKCS1_2048_8192_SHA512,
        Algorithm::PS256 => &ring_signature::RSA_PSS_2048_8192_SHA256,
        Algorithm::PS384 => &ring_signature::RSA_PSS_2048_8192_SHA384,
        Algorithm::PS512 => &ring_signature::RSA_PSS_2048_8192_SHA512,
        Algorithm::ES256 => &ring_signature::ECDSA_P256_SHA256_FIXED,
        Algorithm::ES384 => &ring_signature::ECDSA_P384_SHA384_FIXED,
        Algorithm::EdDSA => &ring_signature::ED25519,
        // HMAC: a hash, as fast either way
        _ => return None,
    })
}

fn new_verifier(algorithm: &Algorithm, key: &DecodingKey) -> Result<Box<dyn JwtVerifier>, Error> {
    let Some(parameters) = parameters(*algorithm) else {
        return (rust_crypto::DEFAULT_PROVIDER.verifier_factory)(algorithm, key);
    };
    if key.family() != algorithm.family() {
        return Err(ErrorKind::InvalidKeyFormat.into());
    }
    Ok(Box::new(RingVerifier {
        algorithm: *algorithm,
        parameters,
        key: key.kind().clone(),
    }))
}

struct RingVerifier {
    algorithm: Algorithm,
    parameters: &'static dyn ring_signature::VerificationAlgorithm,
    key: DecodingKeyKind,
}

impl Verifier<Vec<u8>> for RingVerifier {
    fn verify(&self, message: &[u8], signature: &Vec<u8>) -> Result<(), signature::Error> {
        let verified = match &self.key {
            // PKCS#1 for RSA, the SEC1 point for ECDSA, the 32 bytes of an Ed25519 key
            DecodingKeyKind::SecretOrDer(bytes) => {
                UnparsedPublicKey::new(self.parameters, bytes).verify(message, signature)
            }
            // From a JWK
            DecodingKeyKind::RsaModulusExponent { n, e } => {
                let rsa = match self.algorithm {
                    Algorithm::RS256 => &ring_signature::RSA_PKCS1_2048_8192_SHA256,
                    Algorithm::RS384 => &ring_signature::RSA_PKCS1_2048_8192_SHA384,
                    Algorithm::RS512 => &ring_signature::RSA_PKCS1_2048_8192_SHA512,
                    Algorithm::PS256 => &ring_signature::RSA_PSS_2048_8192_SHA256,
                    Algorithm::PS384 => &ring_signature::RSA_PSS_2048_8192_SHA384,
                    Algorithm::PS512 => &ring_signature::RSA_PSS_2048_8192_SHA512,
                    _ => return Err(signature::Error::new()),
                };
                RsaPublicKeyComponents { n, e }.verify(rsa, message, signature)
            }
        };
        verified.map_err(|_| signature::Error::new())
    }
}

impl JwtVerifier for RingVerifier {
    fn algorithm(&self) -> Algorithm {
        self.algorithm
    }
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{crypto::sign, Algorithm, DecodingKey, EncodingKey};
    use openssl::{
        ec::{EcGroup, EcKey},
        nid::Nid,
        pkey::PKey,
        rsa::Rsa,
    };

    use super::new_verifier;

    /// Signs with `private` and checks with `public` here (not with the process' provider,
    /// which another test may have picked first)
    fn round_trip(algorithm: Algorithm, private: &EncodingKey, public: &DecodingKey) -> bool {
        let signature = sign(b"header.claims", private, algorithm).unwrap();
        let signature = openssl::base64::decode_block(&format!(
            "{}{}",
            signature.replace('-', "+").replace('_', "/"),
            "=".repeat((4 - signature.len() % 4) % 4)
        ))
        .unwrap();
        let verifier = new_verifier(&algorithm, public).unwrap();
        assert!(
            verifier.verify(b"header.claimz", &signature).is_err(),
            "{algorithm:?}: a changed message passed"
        );
        verifier.verify(b"header.claims", &signature).is_ok()
    }

    #[test]
    fn test_every_algorithm_is_checked() {
        let rsa = Rsa::generate(2048).unwrap();
        let private = EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
        let public = DecodingKey::from_rsa_pem(&rsa.public_key_to_pem().unwrap()).unwrap();
        // As a JWK has it
        let components = DecodingKey::from_rsa_raw_components(&rsa.n().to_vec(), &rsa.e().to_vec());
        for algorithm in [
            Algorithm::RS256,
            Algorithm::RS384,
            Algorithm::RS512,
            Algorithm::PS256,
            Algorithm::PS384,
            Algorithm::PS512,
        ] {
            assert!(round_trip(algorithm, &private, &public), "{algorithm:?}");
            assert!(
                round_trip(algorithm, &private, &components),
                "{algorithm:?} (JWK)"
            );
        }

        for (algorithm, curve) in [
            (Algorithm::ES256, Nid::X9_62_PRIME256V1),
            (Algorithm::ES384, Nid::SECP384R1),
        ] {
            let group = EcGroup::from_curve_name(curve).unwrap();
            let ec = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
            let private =
                EncodingKey::from_ec_pem(&ec.private_key_to_pem_pkcs8().unwrap()).unwrap();
            let public = DecodingKey::from_ec_pem(&ec.public_key_to_pem().unwrap()).unwrap();
            assert!(round_trip(algorithm, &private, &public), "{algorithm:?}");
        }

        let ed = PKey::generate_ed25519().unwrap();
        let private = EncodingKey::from_ed_pem(&ed.private_key_to_pem_pkcs8().unwrap()).unwrap();
        let public = DecodingKey::from_ed_pem(&ed.public_key_to_pem().unwrap()).unwrap();
        assert!(round_trip(Algorithm::EdDSA, &private, &public));

        let secret = EncodingKey::from_secret(b"secret");
        assert!(round_trip(
            Algorithm::HS256,
            &secret,
            &DecodingKey::from_secret(b"secret")
        ));
    }

    #[test]
    fn test_rsa_keys_under_2048_bits_are_refused() {
        let rsa = Rsa::generate(1024).unwrap();
        let private = EncodingKey::from_rsa_pem(&rsa.private_key_to_pem().unwrap()).unwrap();
        let public = DecodingKey::from_rsa_pem(&rsa.public_key_to_pem().unwrap()).unwrap();
        assert!(!round_trip(Algorithm::RS256, &private, &public));
    }

    #[test]
    fn test_keys_of_another_family_are_refused() {
        let secret = DecodingKey::from_secret(b"secret");
        assert!(new_verifier(&Algorithm::RS256, &secret).is_err());
        let rsa = Rsa::generate(2048).unwrap();
        let public = DecodingKey::from_rsa_pem(&rsa.public_key_to_pem().unwrap()).unwrap();
        assert!(new_verifier(&Algorithm::ES256, &public).is_err());
    }
}
