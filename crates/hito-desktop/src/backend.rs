use crate::{Error, Result};
use hito_playback::PlayerOwner;
use std::fs::File;
use tokio::{net::TcpListener, sync::oneshot};

pub(super) async fn run(
    listener: TcpListener,
    app: axum::Router,
    storage: hito_core::LibraryStorage,
    player: PlayerOwner,
    mut stopped: oneshot::Receiver<()>,
    library: File,
) -> Result<()> {
    let (stop_http, http_stopped) = oneshot::channel();
    let http = hito_server::serve(listener, app, storage, async {
        let _ = http_stopped.await;
    });
    tokio::pin!(http);
    let finished = tokio::select! {
        _ = &mut stopped => None,
        result = &mut http => Some(result),
    };
    let _ = stop_http.send(());
    let (http, playback) = match finished {
        Some(http) => (http, player.close().await),
        None => tokio::join!(http, player.close()),
    };
    if let Err(error) = playback {
        tracing::error!(event = "desktop_playback_shutdown_failed", error_kind = ?error, "Playback cleanup failed");
    }
    tracing::info!(
        event = "desktop_backend_joined",
        http_success = http.is_ok(),
        playback_success = playback.is_ok(),
        "Private backend cleanup joined"
    );
    drop(library);
    if http.is_ok() && playback.is_ok() {
        Ok(())
    } else {
        Err(Error::Shutdown)
    }
}
