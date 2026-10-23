use crate::{config::RuntimeResult, source_resolution::Resolutions};
use axum::{Extension, Router, extract::ConnectInfo};
use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use std::{future::Future, time::Duration};
use tokio::{net::TcpListener, sync::watch, task::JoinSet};
use tracing::instrument::WithSubscriber;

pub(crate) async fn serve(
    listener: TcpListener,
    app: Router,
    stop: watch::Sender<bool>,
    shutdown: impl Future<Output = ()> + Send,
) -> RuntimeResult<()> {
    tracing::info!(event = "server_listening", address = %listener.local_addr()?, "Hito is listening");
    let resolutions = Resolutions::default();
    let app = app.layer(Extension(resolutions.clone()));
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        resolutions.reap();
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            _ = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept() => {
                let (stream, address) = match accepted {
                    Ok(accepted) => accepted,
                    Err(_) => {
                        tracing::warn!(event = "accept_failed", "Connection could not be accepted");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let service = TowerToHyperService::new(app.clone().layer(Extension(ConnectInfo(address))));
                let mut stopped = stop.subscribe();
                connections.spawn(async move {
                    let builder = http1::Builder::new();
                    let connection = builder.serve_connection(TokioIo::new(stream), service);
                    tokio::pin!(connection);
                    let result = tokio::select! {
                        biased;
                        _ = stopped.changed() => {
                            connection.as_mut().graceful_shutdown();
                            connection.await
                        }
                        result = &mut connection => result,
                    };
                    if result.is_err() {
                        tracing::debug!(event = "connection_closed", "HTTP connection ended before completion");
                    }
                }.with_current_subscriber());
            }
        }
    }
    drop(listener);
    tracing::info!(event = "shutdown_requested", "Shutdown requested");
    let _ = stop.send(true);
    if tokio::time::timeout(Duration::from_secs(5), async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tracing::info!(
            event = "connections_cancelled",
            remaining = connections.len(),
            "Closing remaining HTTP connections"
        );
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }
    resolutions.finish().await?;
    Ok(())
}
