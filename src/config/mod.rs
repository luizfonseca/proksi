pub(crate) struct ConfigPaths {
    pub(crate) cert_dir: String,
    pub(crate) key_dir: String,
    pub(crate) order_dir: String,
    pub(crate) account_dir: String,
}

pub(crate) struct Config {
    paths: ConfigPaths,
}

impl Config {
    pub(crate) fn new() -> Self {
        Config {
            paths: ConfigPaths {
                cert_dir: "./data/certificates".to_string(),
                key_dir: "./data/keys".to_string(),
                order_dir: "./data/orders".to_string(),
                account_dir: "./data/accounts".to_string(),
            },
        }
    }
}
