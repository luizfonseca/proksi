pub struct HclFormat;

impl figment::providers::Format for HclFormat {
    type Error = hcl::Error;

    const NAME: &'static str = "HCL";

    fn from_str<'de, T: serde::de::DeserializeOwned>(string: &'de str) -> Result<T, Self::Error> {
        hcl::de::from_str(string)
    }
}

/// HashiCorp Configuration Language (HCL) provider for figment
pub type Hcl = figment::providers::Data<HclFormat>;
