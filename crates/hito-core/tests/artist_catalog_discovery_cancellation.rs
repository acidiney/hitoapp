#[path = "fixtures/artist_catalog_discovery_service.rs"]
mod discovery;
#[path = "fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
#[path = "fixtures/artist_catalog_playback_provider.rs"]
mod playback_provider;
mod support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    AdoptProviderRecording, ArtistCandidateSearchAvailability, AudioRequest, Error, InstallPlugin,
    MetadataProvider, OfferPluginService, OpenedAudio, UpdateMetadataSettings,
};
use hito_plugin_sdk::ArtistSearchRequest;
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinSet, time::timeout};

#[tokio::test]
async fn cancelling_artist_search_retires_its_held_body_and_allows_same_account_retry() {
    let library = library::Library::prepare().await;
    let audio_directory = tempfile::tempdir().unwrap();
    let original = support::write_wav(&audio_directory.path().join("owned.wav"), 37);
    let mut discovery = discovery::DiscoveryServer::start().await;
    let mut playback = playback_provider::PlaybackServer::start(original.clone()).await;
    let mut searches = JoinSet::new();
    let result = AssertUnwindSafe(exercise(
        &library,
        &discovery,
        &playback,
        &original,
        &mut searches,
    ))
    .catch_unwind()
    .await;
    discovery.release_search();
    searches.shutdown().await;
    let discovery_joined = discovery.finish().await;
    let playback_joined = playback.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(
        discovery_joined,
        "The owned discovery server must join shutdown"
    );
    assert!(
        playback_joined,
        "The owned playback server must join shutdown"
    );
}

async fn exercise(
    library: &library::Library,
    discovery: &discovery::DiscoveryServer,
    playback: &playback_provider::PlaybackServer,
    original: &[u8],
    searches: &mut JoinSet<hito_core::Result<ArtistCandidateSearchAvailability>>,
) {
    let hito = &library.hito;
    let actor = &library.actor;
    let service = hito
        .plugins
        .offer_service(
            &library.admin,
            OfferPluginService {
                name: "Owned playback provider".into(),
                base_url: playback.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: playback_provider::RECORDING_ID.into(),
            },
        )
        .await
        .unwrap();
    let mut sources = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    let source = sources.remove(0);
    assert!(source.ready);
    let service = hito
        .plugins
        .offer_service(
            &library.admin,
            OfferPluginService {
                name: "Owned artist discovery".into(),
                base_url: discovery.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: Some(discovery::TOKEN.into()),
            },
        )
        .await
        .unwrap();
    let settings = hito.catalog.metadata_settings(actor).await.unwrap();
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let policy = hito
        .catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![selected.clone()],
            },
        )
        .await
        .unwrap();
    let context = hito
        .catalog
        .begin_artist_review(actor, &library.recording_id, selected)
        .await
        .unwrap();
    assert!(context.library_id == library.library_id && context.artist_id == library.artist_id);
    assert!(
        context.observation_id == library.observation_id
            && context.source_id.as_deref() == Some(library.source_id.as_str())
            && context.metadata_binding.is_none()
    );
    let before = library.snapshot().await;
    let provider_before = json!({
        "sources": hito.catalog.sources(actor, &recording.id).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, &recording.id, Default::default()).await.unwrap(),
    });
    let gate = discovery.hold_search();
    let request = || ArtistSearchRequest {
        query: library::CREDIT.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };
    let request_catalog = hito.catalog.clone();
    let request_actor = actor.clone();
    let captured = context.clone();
    let search = request();
    let (cancel, cancelled) = oneshot::channel();
    searches.spawn(async move {
        request_catalog
            .search_artist_candidates_until(&request_actor, &captured, search, async {
                let _ = cancelled.await;
            })
            .await
    });
    timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("The owned provider must start a nonempty, incomplete search response");
    assert!(searches.try_join_next().is_none());
    assert!(
        timeout(
            Duration::from_secs(3),
            hito.catalog.refresh_playback_sources_until(
                actor,
                &recording.id,
                std::future::pending()
            ),
        )
        .await
        .expect("Held metadata must leave same-account playback source recovery available")
        .unwrap()
    );
    let audio = timeout(Duration::from_secs(3), async {
        let opened = hito
            .catalog
            .open_audio(actor, &source.id, AudioRequest::default())
            .await
            .unwrap();
        let OpenedAudio::Content(mut content) = opened else {
            panic!("The separate owned ready WAV must open");
        };
        assert_eq!(content.content_type, "audio/wav");
        let mut bytes = Vec::new();
        let mut body = content.body.take().unwrap();
        while let Some(chunk) = body.chunk().await.unwrap() {
            assert!(chunk.len() <= 32_768_usize.saturating_sub(bytes.len()));
            bytes.extend_from_slice(&chunk);
        }
        bytes
    })
    .await
    .expect("The owned audio must finish while the search response remains held");
    assert_eq!(audio, original);
    assert!(
        searches.try_join_next().is_none(),
        "The search must remain pending throughout independent playback"
    );
    cancel.send(()).unwrap();
    let cancelled = timeout(Duration::from_secs(3), searches.join_next())
        .await
        .expect("Cancellation must finish without releasing the provider's response body")
        .expect("The search task must remain owned until joined")
        .expect("The owned search task must not panic")
        .unwrap();
    assert!(matches!(
        cancelled,
        ArtistCandidateSearchAvailability::Cancelled
    ));
    timeout(Duration::from_secs(3), gate.retired())
        .await
        .expect(
            "Cancellation must retire the owned response body before release or server shutdown",
        );
    assert!(searches.is_empty());
    assert!(library.snapshot().await == before);

    discovery.release_search();
    let result = timeout(
        Duration::from_secs(3),
        hito.catalog.search_artist_candidates_until(
            actor,
            &context,
            request(),
            std::future::pending(),
        ),
    )
    .await
    .expect("The same account must immediately regain metadata admission after cancellation")
    .unwrap();
    let ArtistCandidateSearchAvailability::Available {
        context: returned,
        page,
    } = result
    else {
        panic!("The unchanged current context must remain usable after cancellation");
    };
    assert!(*returned == context);
    page.validate_for(&request()).unwrap();
    assert!(page.snapshot.as_deref() == Some(discovery::SNAPSHOT));
    assert!(
        page.items.len() == 2
            && page.items[0].artist.id == discovery::FIRST_ARTIST
            && page.items[1].artist.id == discovery::SECOND_ARTIST
    );
    assert!(library.snapshot().await == before);
    assert!(
        json!({
            "sources": hito.catalog.sources(actor, &recording.id).await.unwrap(),
            "metadata": hito.catalog.recording_metadata(actor, &recording.id, Default::default()).await.unwrap(),
        }) == provider_before
    );
    assert!(
        serde_json::to_value(hito.catalog.metadata_settings(actor).await.unwrap()).unwrap()
            == serde_json::to_value(policy).unwrap()
    );
    for id in [
        &library.recording_id,
        &library.other_recording_id,
        &recording.id,
    ] {
        assert!(
            hito.catalog
                .recording_artist_associations(actor, id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            hito.downloads.status(actor, id).await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(discovery.unexpected(), 0);
    let observed = discovery.requests();
    assert_eq!(observed.len(), 5);
    assert!(observed[0].method == Method::GET && observed[0].path == "/prefix/v1/manifest");
    let (pairs, remainder) = observed[1..].as_chunks::<2>();
    assert!(remainder.is_empty());
    for pair in pairs {
        assert!(
            pair[0].method == Method::GET
                && pair[0].path == "/prefix/v1/catalog/discovery/capabilities"
                && pair[0].body.is_null()
        );
        assert!(
            pair[1].method == Method::POST
                && pair[1].path == "/prefix/v1/catalog/artists/search"
                && pair[1].body == serde_json::to_value(request()).unwrap(),
            "Only the SDK search envelope may reach the provider during the cancelled request or retry"
        );
    }
    for request in observed {
        assert!(
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                == Some("Bearer private-artist-discovery-service-token")
        );
        assert!(request.headers.get("cookie").is_none());
    }
    assert_eq!(
        playback.counts(),
        playback_provider::Counts {
            manifests: 1,
            details: 1,
            sources: 2,
            audio: 1,
            unexpected: 0,
        }
    );
}
