use axum::Router;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, task::JoinSet};
use tracing::Instrument;

const MAX_CONNECTIONS: usize = 16;
const MAX_HEADER_BYTES: usize = 16_384;

pub(super) async fn serve(
    listener: TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()>,
) -> Result<(), &'static str> {
    let started = Instant::now();
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    let served = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break Ok(()),
            completed = connections.join_next(), if !connections.is_empty() => {
                if !matches!(completed, Some(Ok(()))) {
                    break Err("http_connection_task_failed");
                }
            }
            accepted = listener.accept() => {
                let (socket, _) = match accepted {
                    Ok(socket) => socket,
                    Err(_) => break Err("accept_failed"),
                };
                if connections.len() >= MAX_CONNECTIONS {
                    tracing::debug!(
                        event = "torbox_http_connection_rejected",
                        outcome = "capacity",
                        count = 1_u64,
                        elapsed_ms = started.elapsed().as_millis() as u64
                    );
                    drop(socket);
                    continue;
                }
                let app = app.clone();
                connections.spawn(async move {
                    let mut protocol = hyper::server::conn::http1::Builder::new();
                    protocol.timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(14))
                        .max_headers(32)
                        .max_header_size(MAX_HEADER_BYTES)
                        .max_buf_size(MAX_HEADER_BYTES);
                    let _ = protocol.serve_connection(
                        TokioIo::new(socket), TowerToHyperService::new(app),
                    ).await;
                }.in_current_span());
            }
        }
    };
    drop(listener);
    let count = connections.len() as u64;
    connections.abort_all();
    let mut joined = Ok(());
    while let Some(completed) = connections.join_next().await {
        if let Err(error) = completed
            && !error.is_cancelled()
        {
            joined = Err("http_connection_task_failed");
        }
    }
    tracing::info!(
        event = "torbox_http_connections_joined",
        outcome = "joined",
        count,
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    served.and(joined)
}
