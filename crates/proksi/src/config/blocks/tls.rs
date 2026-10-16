use std::fmt;

use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};

use crate::config::{host::HostTls, RouteSslPath};

/// `tls { ... }` in a host
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsBlock {
    /// Certificate files, used instead of Let's Encrypt
    pub certificate: Option<RouteSslPath>,
    /// Use a self-signed certificate if no other certificate can be obtained
    #[serde(default)]
    pub self_signed_on_failure: bool,
}

/// `tls = false` / `tls = true`, or `tls { ... }`
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum TlsSetting {
    Enabled(bool),
    Settings(TlsBlock),
}

impl TlsSetting {
    pub fn to_host_tls(&self) -> HostTls {
        match self {
            TlsSetting::Enabled(enabled) => HostTls {
                enabled: *enabled,
                ..HostTls::default()
            },
            TlsSetting::Settings(block) => HostTls {
                enabled: true,
                certificate: block.certificate.clone(),
                self_signed_on_failure: block.self_signed_on_failure,
            },
        }
    }
}

/// Written by hand (rather than `#[serde(untagged)]`) so that errors inside `tls { ... }`,
/// such as unknown attributes, are reported as they are
impl<'de> Deserialize<'de> for TlsSetting {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TlsVisitor;

        impl<'de> Visitor<'de> for TlsVisitor {
            type Value = TlsSetting;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a boolean or a tls block")
            }

            fn visit_bool<E: de::Error>(self, enabled: bool) -> Result<TlsSetting, E> {
                Ok(TlsSetting::Enabled(enabled))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<TlsSetting, A::Error> {
                TlsBlock::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(TlsSetting::Settings)
            }
        }

        deserializer.deserialize_any(TlsVisitor)
    }
}

#[cfg(test)]
mod tests {
    use figment::providers::{Format, Yaml};
    use serde::Deserialize;

    use crate::config::hcl::HclFormat;

    use super::{TlsBlock, TlsSetting};

    #[derive(Deserialize)]
    struct Doc {
        tls: TlsSetting,
    }

    fn hcl(src: &str) -> Result<TlsSetting, String> {
        HclFormat::from_str::<Doc>(src)
            .map(|doc| doc.tls)
            .map_err(|err| err.to_string())
    }

    #[test]
    fn test_tls_can_be_disabled() {
        assert_eq!(hcl("tls = false\n"), Ok(TlsSetting::Enabled(false)));
        assert_eq!(hcl("tls = true\n"), Ok(TlsSetting::Enabled(true)));
        assert_eq!(
            Yaml::from_str::<Doc>("tls: false\n")
                .map(|doc| doc.tls)
                .ok(),
            Some(TlsSetting::Enabled(false))
        );
    }

    #[test]
    fn test_tls_block() {
        assert_eq!(
            hcl("tls {\n  self_signed_on_failure = true\n}\n"),
            Ok(TlsSetting::Settings(TlsBlock {
                certificate: None,
                self_signed_on_failure: true,
            }))
        );
    }

    #[test]
    fn test_tls_block_with_unknown_attribute() {
        let err = hcl("tls {\n  self_signed = true\n}\n").unwrap_err();
        assert!(err.contains("unknown field `self_signed`"), "{err}");
    }

    #[test]
    fn test_tls_setting_to_host_tls() {
        let disabled = TlsSetting::Enabled(false).to_host_tls();
        assert!(!disabled.enabled);
        assert!(disabled.certificate.is_none());

        let settings = TlsSetting::Settings(TlsBlock {
            certificate: None,
            self_signed_on_failure: true,
        })
        .to_host_tls();
        assert!(settings.enabled);
        assert!(settings.self_signed_on_failure);
    }
}
