use crate::{Error, Result};
use hito_playback::PlayerOwner;
use std::fs::File;
use tokio::{net::TcpListener, sync::oneshot};

pub(super) async fn run(
    listener: TcpListener,
    app: axum::Router,
    services: hito_core::Services,
    player: PlayerOwner,
    mut stopped: oneshot::Receiver<()>,
    library: File,
) -> Result<()> {
    let (stop_http, http_stopped) = oneshot::channel();
    let (stop_conditions, conditions_stopped) = oneshot::channel();
    let catalog = services.catalog.clone();
    let conditions = tokio::spawn(crate::preparation_conditions::watch(
        crate::preparation_conditions::system(),
        move |held| catalog.hold_background_preparation(held),
        conditions_stopped,
    ));
    let http = hito_server::serve(listener, app, services, async {
        let _ = http_stopped.await;
    });
    tokio::pin!(http);
    let finished = tokio::select! {
        _ = &mut stopped => None,
        result = &mut http => Some(result),
    };
    let _ = stop_http.send(());
    let _ = stop_conditions.send(());
    let (http, playback) = match finished {
        Some(http) => (http, player.close().await),
        None => tokio::join!(http, player.close()),
    };
    let _ = conditions.await;
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
