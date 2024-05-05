use std::{collections::HashMap, hash::Hash, thread::sleep, time::Duration};

use async_trait::async_trait;
use bollard::{service::ListServicesOptions, Docker};
use pingora::{server::ShutdownWatch, services::background::BackgroundService};

pub fn create_client() -> DockerClient {
    let docker = Docker::connect_with_local_defaults();
    DockerClient {
        inner: docker.unwrap(),
    }
}

pub struct DockerClient {
    inner: Docker,
}

impl DockerClient {
    pub async fn start_service(&self) -> () {
        let mut default_filters = HashMap::new();
        default_filters.insert("label", vec!["proksi.host", "proksi.port"]);

        loop {
            println!("Docker running");
            self.list_containers(default_filters.clone()).await;

            sleep(Duration::from_secs(5));
        }
    }

    async fn list_containers<T>(&self, filters: HashMap<T, Vec<T>>) -> ()
    where
        T: Into<String> + Hash + serde::ser::Serialize + Eq,
    {
        let services = self
            .inner
            .list_services(Some(ListServicesOptions {
                filters,
                status: true,
            }))
            .await;

        if services.is_err() {
            println!("Error listing services");
            return;
        }

        for service in services.unwrap() {
            println!("Found service {}", service.spec.unwrap().name.unwrap());
        }
    }
}

#[async_trait]
impl BackgroundService for DockerClient {
    async fn start(&self, _shutdown: ShutdownWatch) -> () {
        self.start_service().await;
    }
}
