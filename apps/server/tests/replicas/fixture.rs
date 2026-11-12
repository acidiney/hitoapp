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

pub struct ClosedLibrary {
    directory: tempfile::TempDir,
    host: String,
}

impl ClosedLibrary {
    pub async fn open(self) -> Library {
        let Self { directory, host } = self;
        let services = open(directory.path(), &host).await;
        let session = services.identity.login(credentials("admin")).await.unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        Library {
            directory,
            services,
            actor,
            token: session.token,
            host,
        }
    }
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
        self.close().await.open().await
    }

    pub async fn close(self) -> ClosedLibrary {
        let Self {
            directory,
            services,
            host,
            ..
        } = self;
        services.replicas.close().await.unwrap();
        services.catalog_replication.close().await.unwrap();
        drop(services);
        ClosedLibrary { directory, host }
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

pub struct ClosedHost {
    library: ClosedLibrary,
    endpoint: String,
}

impl ClosedHost {
    pub async fn start(self) -> Host {
        let library = self.library.open().await;
        let address = self.endpoint.strip_prefix("http://").unwrap();
        let listener = TcpListener::bind(address).await.unwrap();
        Host::serve(library, listener).await
    }
}

impl Host {
    pub async fn start() -> Self {
        let library = Library::new("owned-authoritative-host").await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        Self::serve(library, listener).await
    }

    async fn serve(library: Library, listener: TcpListener) -> Self {
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

    pub async fn stop(mut self) -> ClosedHost {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(serving) = self.serving.take() {
            tokio::time::timeout(Duration::from_secs(3), serving)
                .await
                .unwrap()
                .unwrap();
        }
        let endpoint = self.endpoint.clone();
        let replacement = tempfile::tempdir().unwrap();
        self.library.services.replicas.close().await.unwrap();
        self.library
            .services
            .catalog_replication
            .close()
            .await
            .unwrap();
        let directory = std::mem::replace(&mut self.library.directory, replacement);
        let host = self.library.host.clone();
        drop(self);
        ClosedHost {
            library: ClosedLibrary { directory, host },
            endpoint,
        }
    }

    pub async fn close(self) {
        drop(self.stop().await);
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
