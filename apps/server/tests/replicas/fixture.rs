use hito_core::{Actor, ConnectReplica, CoreConfig, Credentials, HostDeviceId, Services};
use std::{path::Path, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

pub const PASSWORD: &str = "owned replica acceptance password";

pub fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

pub struct Library {
    pub directory: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    host: String,
}

impl Library {
    pub async fn new(host: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let services = open(directory.path(), host).await;
        let session = services.identity.setup(credentials("admin")).await.unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        Self {
            directory,
            services,
            actor,
            token: session.token,
            host: host.into(),
        }
    }

    pub async fn reopen(self) -> Self {
        let Self {
            directory,
            services,
            host,
            ..
        } = self;
        services.replicas.close().await.unwrap();
        drop(services);
        let services = open(directory.path(), &host).await;
        let session = services.identity.login(credentials("admin")).await.unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        Self {
            directory,
            services,
            actor,
            token: session.token,
            host,
        }
    }
}

async fn open(root: &Path, host: &str) -> Services {
    hito_core::open(CoreConfig::new(
        root.join("hito.db"),
        root.join("music"),
        HostDeviceId::new(host).unwrap(),
    ))
    .await
    .unwrap()
}

pub struct Host {
    pub library: Library,
    pub endpoint: String,
    pub faults: super::faults::Faults,
    stop: Option<oneshot::Sender<()>>,
    serving: Option<JoinHandle<()>>,
}

impl Host {
    pub async fn start() -> Self {
        let library = Library::new("owned-authoritative-host").await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let config = hito_server::Config {
            origin: "https://canonical-hito.invalid".into(),
            ..Default::default()
        };
        let faults = super::faults::Faults::default();
        let app = hito_server::router_with_config(library.services.clone(), config).layer(
            axum::middleware::from_fn_with_state(faults.clone(), super::faults::intercept),
        );
        let (stop, stopped) = oneshot::channel();
        let serving = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        Self {
            library,
            endpoint,
            faults,
            stop: Some(stop),
            serving: Some(serving),
        }
    }

    pub fn request(&self, username: &str) -> ConnectReplica {
        ConnectReplica {
            endpoint: self.endpoint.clone(),
            credentials: credentials(username),
        }
    }

    pub async fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(serving) = self.serving.take() {
            tokio::time::timeout(Duration::from_secs(3), serving)
                .await
                .unwrap()
                .unwrap();
        }
        self.library.services.replicas.close().await.unwrap();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(serving) = self.serving.take() {
            serving.abort();
        }
    }
}
