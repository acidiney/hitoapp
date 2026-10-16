use crate::{Error, Paths, Result, access::Access, storage};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tracing::{Instrument, instrument::WithSubscriber};
use url::Url;

type Serving = JoinHandle<Result<()>>;

pub struct LocalHost {
    entry: Url,
    stop: Option<oneshot::Sender<()>>,
    serving: Option<Serving>,
}

impl LocalHost {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn start(paths: Paths) -> Result<Self> {
        let lock = storage::claim(&paths.data)?;
        let mut config = hito_server::RuntimeConfig::load(
            paths.data.join("hito.db"),
            paths.music,
            paths.web,
            None,
        )
        .await
        .map_err(|_| Error::Startup)?;
        let services = hito_core::open(config.core).await.map_err(|error| {
            tracing::error!(
                event = "desktop_core_failed",
                error_kind = error.code(),
                "Local services could not start"
            );
            Error::Startup
        })?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| Error::Startup)?;
        let address = listener.local_addr().map_err(|_| Error::Startup)?;
        let origin = Url::parse(&format!("http://{address}")).map_err(|_| Error::Startup)?;
        config.http.origin = origin.origin().ascii_serialization();
        let (access, entry) = Access::new(&origin);
        let storage = services.storage.clone();
        #[cfg(feature = "device")]
        let (player, owner) = hito_playback::Player::native(services.clone());
        #[cfg(not(feature = "device"))]
        let (player, owner) = hito_playback::Player::start(services.clone());
        let app = access.protect(hito_server::router_with_player(
            services,
            config.http,
            player,
        ));
        let (stop, stopped) = oneshot::channel();
        let serving = tokio::spawn(
            crate::backend::run(listener, app, storage, owner, stopped, lock)
                .instrument(tracing::Span::current())
                .with_current_subscriber(),
        );
        tracing::info!(
            event = "desktop_backend_ready",
            "Private local backend started"
        );
        Ok(Self {
            entry,
            stop: Some(stop),
            serving: Some(serving),
        })
    }

    pub fn entry_url(&self) -> Url {
        self.entry.clone()
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(serving) = self.serving.take() else {
            return Ok(());
        };
        match serving.await {
            Ok(Ok(())) => {
                tracing::info!(event = "desktop_backend_stopped", "Local backend stopped");
                Ok(())
            }
            _ => Err(Error::Shutdown),
        }
    }
}

impl Drop for LocalHost {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
