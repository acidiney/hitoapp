#[path = "podcast_browser/controls.rs"]
mod controls;
#[path = "../../hito-core/tests/fixtures/podcast_audio_service.rs"]
#[allow(dead_code)]
mod fixture;
#[path = "podcast_browser/wav.rs"]
mod wav;

use controls::OwnedFixture;
use hito_core::{CoreConfig, HostDeviceId, PodcastNetworkConfig, SharedDirectoryConfig};
use std::{future::Future, path::Path, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Mutex};
use tracing_subscriber::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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
    let audio = wav::audio();
    tokio::fs::write(music.join("Owned original.wav"), &audio).await?;
    let fixture = Arc::new(Mutex::new(OwnedFixture::start(audio).await));
    let result = serve(directory, music, fixture.clone(), shutdown).await;
    if !fixture.lock().await.stop().await {
        return Err(std::io::Error::other("the owned RSS fixture did not join").into());
    }
    result
}

async fn serve(
    directory: &Path,
    music: std::path::PathBuf,
    fixture: Arc<Mutex<OwnedFixture>>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let mut core = CoreConfig::with_shared_directories(
        directory.join("hito.db"),
        HostDeviceId::new("owned-podcast-browser")?,
        vec![SharedDirectoryConfig {
            id: "owned-podcast-original".into(),
            name: "Owned original folder".into(),
            root: music,
        }],
    );
    core.podcast_network =
        PodcastNetworkConfig::new(vec![fixture.lock().await.origin().to_owned()])?;
    let services = hito_core::open(core).await?;
    let listener = TcpListener::bind("0.0.0.0:8080").await?;
    let app = hito_server::router_with_config(services.clone(), hito_server::Config::default())
        .merge(controls::routes(fixture));
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
