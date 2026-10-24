use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use pingora::{
    server::{ListenFds, ShutdownWatch},
    services::Service,
};
use tokio::time;
use tracing::info;

use crate::{config::Config, letsencrypt::http01::Acme};

/// A service that handles the creation of certificates using the Let's Encrypt API
pub struct LetsencryptService {
    acme: Acme,
}

impl LetsencryptService {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            acme: Acme::new(config),
        }
    }
}

#[async_trait]
impl Service for LetsencryptService {
    async fn start_service(
        &mut self,
        _fds: Option<ListenFds>,
        mut _shutdown: ShutdownWatch,
        _listeners_per_fd: usize,
    ) {
        if self.acme.config.lets_encrypt.enabled.is_some_and(|v| !v) {
            // Nothing to do, lets encrypt is disabled
            return;
        }
        info!("started LetsEncrypt service");

        // Retry until the account is available (e.g. network not ready yet)
        let account = loop {
            match self.acme.get_account_with_lease().await {
                Ok(account) => break account,
                Err(err) => {
                    tracing::error!("failed to setup LetsEncrypt, retrying in 60s: {err}");
                    time::sleep(Duration::from_secs(60)).await;
                }
            }
        };

        let _ = tokio::join!(
            self.acme.watch_for_route_changes(&account),
            self.acme.check_for_certificates_expiration(&account)
        );
    }

    fn name(&self) -> &'static str {
        "lets_encrypt_service"
    }

    fn threads(&self) -> Option<usize> {
        Some(1)
    }
}
