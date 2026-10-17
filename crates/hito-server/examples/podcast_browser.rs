#[path = "../../hito-core/tests/fixtures/podcast_audio_service.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "podcast_browser/wav.rs"]
mod wav;

use axum::{Json, routing::get};
use fixture::PodcastAudioServer;
use hito_core::{CoreConfig, HostDeviceId, PodcastNetworkConfig};
use serde::Serialize;
use std::{
    future::Future,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::Mutex};
use tracing_subscriber::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Serialize)]
struct FixtureStatus {
    kind: &'static str,
    feed_url: String,
    feed_requests: usize,
    enclosure_requests: usize,
    unexpected_requests: usize,
}

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(
            hito_server::application_log,
        ))
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hito_server=info,hito_core=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run(directory.path()));
    runtime.shutdown_timeout(Duration::from_secs(3));
    drop(directory);
    result
}

async fn run(directory: &Path) -> Result<()> {
    let shutdown = shutdown_signal()?;
    let music = directory.join("music");
    tokio::fs::create_dir(&music).await?;
    let fixture = Arc::new(Mutex::new(
        PodcastAudioServer::start_with_ranges(wav::audio()).await,
    ));
    let result = serve(directory, music, fixture.clone(), shutdown).await;
    if !fixture.lock().await.finish().await {
        return Err(std::io::Error::other("the owned RSS fixture did not join").into());
    }
    result
}

async fn serve(
    directory: &Path,
    music: std::path::PathBuf,
    fixture: Arc<Mutex<PodcastAudioServer>>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let mut core = CoreConfig::new(
        directory.join("hito.db"),
        music,
        HostDeviceId::new("owned-podcast-browser")?,
    );
    core.podcast_network = PodcastNetworkConfig::new(vec![fixture.lock().await.origin.clone()])?;
    let services = hito_core::open(core).await?;
    let listener = TcpListener::bind("0.0.0.0:8080").await?;
    let app = hito_server::router_with_config(services.clone(), hito_server::Config::default())
        .route(
            "/__podcast_fixture",
            get(move || {
                let fixture = fixture.clone();
                async move {
                    let started = Instant::now();
                    let request_id = uuid::Uuid::new_v4();
                    let fixture = fixture.lock().await;
                    let status = FixtureStatus {
                        kind: "hito-owned-podcast-browser",
                        feed_url: fixture.feed_url.clone(),
                        feed_requests: fixture.feed_requests(),
                        enclosure_requests: fixture.enclosure_requests(),
                        unexpected_requests: fixture.unexpected(),
                    };
                    tracing::debug!(
                        target: "hito_server::podcast_browser_fixture",
                        event = "fixture_status",
                        outcome = "read",
                        %request_id,
                        feed_requests = status.feed_requests,
                        enclosure_requests = status.enclosure_requests,
                        unexpected_requests = status.unexpected_requests,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "Owned podcast fixture status read"
                    );
                    Json(status)
                }
            }),
        );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let serving = hito_server::serve(listener, app, services, async {
        let _ = stopped.await;
    });
    tokio::pin!(serving);
    tokio::select! {
        result = &mut serving => result,
        _ = shutdown => {
            let _ = stop.send(());
            tokio::time::timeout(Duration::from_secs(8), &mut serving)
                .await
                .map_err(|_| std::io::Error::other("the podcast browser host did not join"))?
        }
    }
}

fn shutdown_signal() -> std::io::Result<impl Future<Output = ()> + Send> {
    #[cfg(unix)]
    let (mut interrupt, mut terminate) = {
        use tokio::signal::unix::{SignalKind, signal};
        (
            signal(SignalKind::interrupt())?,
            signal(SignalKind::terminate())?,
        )
    };
    Ok(async move {
        #[cfg(unix)]
        tokio::select! {
            _ = interrupt.recv() => {},
            _ = terminate.recv() => {},
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    })
}
