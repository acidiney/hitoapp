use hito_core::{CoreConfig, HostDeviceId, Services};
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};

pub async fn open(directory: &Path, host: &str) -> Services {
    hito_core::open(CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new(host).unwrap(),
    ))
    .await
    .unwrap()
}

pub struct Host {
    pub address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<bool>>,
}

impl Host {
    pub async fn start(services: Services) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = hito_server::Config {
            origin: format!("http://{address}"),
            ..Default::default()
        };
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            hito_server::serve(
                listener,
                hito_server::router_with_config(services.clone(), config),
                services,
                async {
                    let _ = stopped.await;
                },
            )
            .await
            .is_ok()
        });
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }

    pub fn request_shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    pub async fn join(&mut self) -> bool {
        let Some(mut task) = self.task.take() else {
            return true;
        };
        match tokio::time::timeout(Duration::from_secs(4), &mut task).await {
            Ok(result) => {
                matches!(result, Ok(true)) && TcpStream::connect(self.address).await.is_err()
            }
            Err(_) => {
                task.abort();
                let _ = task.await;
                false
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.request_shutdown();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
