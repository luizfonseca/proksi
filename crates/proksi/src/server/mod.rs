use std::sync::Arc;
use pingora::server::{Server, configuration::Opt};
use crate::config::Config;
use crate::proxy_service::builder::ProxyServiceBuilder;
use crate::services::BackgroundFunctionService;
use crate::services::logger::ProxyLoggerReceiver;
use crate::MsgProxy;

pub mod manager;

pub struct ProxyServerManager {
    config: Arc<Config>,
    server: Server,
}

impl ProxyServerManager {
    pub fn new(config: Arc<Config>) -> Result<Self, anyhow::Error> {
        let pingora_opts = Opt {
            daemon: config.daemon,
            upgrade: config.upgrade,
            conf: None,
            nocapture: false,
            test: false,
        };

        let mut server = Server::new(Some(pingora_opts))?;
        server.bootstrap();

        Ok(Self { config, server })
    }
    
    pub fn setup_services(&mut self) -> Result<(), anyhow::Error> {
        // Setup proxy services
        let builder = ProxyServiceBuilder::new(self.config.clone());
        let proxy_services = builder.build_services();
        
        for proxy_service in proxy_services {
            proxy_service.add_to_server(&mut self.server)?;
        }
        
        // Setup background services
        let (sender, _receiver) = tokio::sync::broadcast::channel::<MsgProxy>(10);
        let background_service = BackgroundFunctionService::new(self.config.clone(), sender);
        self.server.add_service(background_service);
        
        // Note: Logger service will be set up separately in main.rs
        // as it requires different channel management
        
        Ok(())
    }
    
    pub fn add_logger_service(&mut self, logger_service: ProxyLoggerReceiver) {
        self.server.add_service(logger_service);
    }
    
    pub fn run_forever(self) -> ! {
        self.server.run_forever()
    }
}