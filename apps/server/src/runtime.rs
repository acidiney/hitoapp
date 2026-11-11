mod artwork;
mod community;
mod downloads;
mod follows;
mod indexing;
mod matching;
mod plugin_offers;
mod podcasts;
mod polling;
mod retention;
mod source_preparation;

use crate::{RuntimeConfig, config::RuntimeResult, router_with_config};
pub use plugin_offers::offer_configured_plugin_services;
use std::future::Future;
use tracing::{Instrument, instrument::WithSubscriber};
use tracing_subscriber::prelude::*;

pub async fn run(
    config: RuntimeConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "hito_server=info,hito_core=info".into());
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(
            crate::telemetry::application_log,
        ))
        .with(filter);
    if config.json_logs {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
                    .json(),
            )
            .try_init()?;
    } else {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE),
            )
            .try_init()?;
    }
    if config.community_config_invalid {
        tracing::warn!(
            event = "community_configuration_disabled",
            reason = "invalid_host_configuration",
            "Community sharing is disabled because the trusted host configuration is invalid"
        );
    }
    if config.community_ranking_config_invalid {
        tracing::warn!(
            event = "community_ranking_configuration_disabled",
            reason = "invalid_host_configuration",
            "Community source rankings are disabled because the trusted host configuration is invalid"
        );
    }
    tracing::info!(event = "server_starting", "Starting Hito");
    let services = hito_core::open(config.core).await?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    let (stop_offers, offers_stopped) = tokio::sync::watch::channel(false);
    let offers = tokio::spawn(
        offer_configured_plugin_services(
            services.plugins.clone(),
            config.plugin_services,
            offers_stopped,
        )
        .instrument(tracing::Span::current())
        .with_current_subscriber(),
    );
    let result = serve(
        listener,
        router_with_config(services.clone(), config.http),
        services,
        shutdown,
    )
    .await;
    let _ = stop_offers.send(true);
    let _ = offers.await;
    result
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    services: hito_core::Services,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let (cancel, cancelled) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(
        serve_owned(listener, app, services, async move {
            tokio::select! {
                _ = shutdown => {},
                _ = cancelled => {},
            }
        })
        .instrument(tracing::Span::current())
        .with_current_subscriber(),
    );
    let result = serving.await?;
    drop(cancel);
    result
}

async fn serve_owned(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    services: hito_core::Services,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> RuntimeResult<()> {
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let replicas = services.replicas.clone();
    let snapshots = services.catalog_replication.clone();
    let playback = services.playback.clone();
    let mut capability_stop = stopped.clone();
    let capability_shutdown = async move {
        let _ = capability_stop.wait_for(|stopped| *stopped).await;
        tokio::join!(replicas.close(), snapshots.close(), playback.close())
    };
    let indexing = indexing::worker(services.storage.clone(), stopped.clone());
    let watching = indexing::folder_watching(services.storage.clone(), stopped.clone());
    let retention = retention::worker(services.storage.clone(), stopped.clone());
    let artwork = artwork::worker(services.catalog.clone(), stopped.clone());
    let covers = artwork::cover_art_worker(services.catalog.clone(), stopped.clone());
    let matching = matching::worker(services.artist_catalog.clone(), stopped.clone());
    let reports = community::worker(services.community, stopped.clone());
    let preparation_catalog = services.catalog;
    let preparation_stop = stopped.clone();
    let preparations = async move {
        tokio::join!(
            source_preparation::worker(preparation_catalog.clone(), preparation_stop.clone(), 0),
            source_preparation::worker(preparation_catalog, preparation_stop, 1),
        );
    };
    let download_storage = services.downloads.clone();
    let downloads = services.downloads;
    let download_stop = stopped.clone();
    let transfers = async move {
        tokio::join!(
            downloads::worker(downloads.clone(), download_stop.clone(), 0),
            downloads::worker(downloads.clone(), download_stop.clone(), 1),
            downloads::reconcile(downloads, download_stop),
        );
    };
    let podcasts = services.podcasts;
    let podcast_stop = stopped.clone();
    let refreshes = async move {
        tokio::join!(
            podcasts::worker(podcasts.clone(), podcast_stop.clone(), 0),
            podcasts::worker(podcasts, podcast_stop, 1),
        );
    }
    .instrument(tracing::Span::current())
    .with_current_subscriber();
    let collection = services.collection;
    let follows = async move {
        tokio::join!(
            follows::worker(collection.clone(), stopped.clone(), 0),
            follows::worker(collection.clone(), stopped.clone(), 1),
            follows::worker(collection.clone(), stopped.clone(), 2),
            follows::worker(collection, stopped, 3),
        );
    };
    let http = crate::connections::serve(listener, app, stop, shutdown);
    let (result, (), (), (), (), (), (), (), (), (), (), (), capabilities_closed) = tokio::join!(
        http,
        indexing,
        watching,
        retention,
        artwork,
        matching,
        covers,
        follows,
        transfers,
        preparations,
        refreshes,
        reports,
        capability_shutdown
    );
    let (downloads_finished, indexing_finished) = tokio::join!(
        download_storage.finish_downloads(),
        services.storage.finish_indexing(),
    );
    let (replicas_closed, snapshots_closed, playback_closed) = capabilities_closed;
    replicas_closed?;
    snapshots_closed?;
    playback_closed?;
    downloads_finished?;
    indexing_finished?;
    tracing::info!(event = "server_stopped", "Hito stopped");
    result
}
