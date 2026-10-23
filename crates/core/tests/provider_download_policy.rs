use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    Actor, AdoptProviderRecording, AudioRequest, DownloadRequest, Error, InstallPlugin,
    OfferPluginService, OpenedAudio, PreviewPluginInstallation, ProviderSearch, Services,
};
use serde_json::json;
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

const REMOTE_RECORDING: &str = "private-policy-recording";
const REMOTE_SOURCE: &str = "private-policy-source";
const TITLE: &str = "private-policy-title";
const ARTIST: &str = "private-policy-artist";
const QUERY: &str = "private-policy-query";

#[tokio::test]
async fn a_playback_only_provider_can_stream_but_cannot_create_a_persistent_download() {
    let (_directory, hito, actor, _library, music) = tokio::time::timeout(
        Duration::from_secs(10),
        support::library("provider-download-policy"),
    )
    .await
    .expect("the temporary library must open within its deadline");
    let original_path = music.join("original.wav");
    let original = support::write_wav(&original_path, 421);
    let server = fixture::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(&hito, &actor, &server, &original)).catch_unwind(),
    )
    .await;
    let settled =
        tokio::time::timeout(Duration::from_secs(5), hito.downloads.finish_downloads()).await;
    let joined = tokio::time::timeout(Duration::from_secs(2), server.shutdown()).await;
    assert!(
        matches!(settled, Ok(Ok(()))),
        "managed filesystem work must settle on success or panic"
    );
    assert!(joined.is_ok(), "the owned provider must shut down and join");
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "the user-owned original bytes must remain unchanged"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public provider policy workflow exceeded its deadline"),
    }
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    original: &[u8],
) {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.policy-fixture",
                "name": "Owned playback-only fixture",
                "protocol_version": 1,
                "capabilities": ["search", "sources"],
                "content_policy": {"source_persistence": "playback_only"}
            }),
        )
        .await;
    let details = json!({
        "id": REMOTE_RECORDING,
        "title": TITLE,
        "artist": ARTIST,
        "duration_ms": 1000
    });
    server
        .reply("v1/search", json!({"items": [details.clone()]}))
        .await;
    server.reply("v1/recordings/detail", details).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": REMOTE_SOURCE, "label": "Playback", "ready": true}]}),
        )
        .await;
    server
        .audio(
            StatusCode::OK,
            HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/wav".parse().unwrap(),
            )]),
            original.to_vec(),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned playback-only fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let preview = hito
        .plugins
        .preview_installation(
            actor,
            PreviewPluginInstallation {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: Some(preview.digest),
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .expect("a valid playback-only manifest must install");
    let page = hito
        .catalog
        .search_provider(
            actor,
            &installation.id,
            ProviderSearch {
                installation_revision: installation.revision,
                query: QUERY.into(),
                cursor: None,
                limit: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].id == REMOTE_RECORDING);
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: page.items[0].id.clone(),
            },
        )
        .await
        .unwrap();
    let sources = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready);
    assert!(server.requests("v1/audio").await.is_empty());
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert!(server.requests("v1/audio").await.is_empty());

    let OpenedAudio::Content(mut audio) = hito
        .catalog
        .open_audio(actor, &sources[0].id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("the playback-only source must remain streamable");
    };
    assert_eq!(audio.content_type, "audio/wav");
    let mut received = Vec::new();
    let body = audio.body.as_mut().expect("playback must contain audio");
    while let Some(chunk) = body.chunk().await.unwrap() {
        received.extend_from_slice(&chunk);
    }
    assert!(
        received == original,
        "playback must return the exact owned WAV"
    );
    drop(audio);
    assert_eq!(server.requests("v1/audio").await.len(), 1);

    let rejection = hito
        .downloads
        .create(
            actor,
            &recording.id,
            DownloadRequest {
                request_id: Uuid::new_v4(),
            },
        )
        .await;
    let message = match rejection {
        Err(Error::Conflict(message)) => message,
        Err(error) => panic!("expected a policy conflict, received {}", error.code()),
        Ok(_) => panic!("a playback-only source must reject persistent download admission"),
    };
    assert!(message.contains("playback only"));
    assert!(message.len() <= 256);
    for private in [
        REMOTE_RECORDING,
        REMOTE_SOURCE,
        TITLE,
        ARTIST,
        QUERY,
        &server.url,
    ] {
        assert!(
            !message.contains(private),
            "the conflict must omit private provider data"
        );
    }
    assert!(
        !message.contains("://"),
        "the conflict must omit transport URLs"
    );
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert_eq!(server.requests("v1/audio").await.len(), 1);
    let after = hito.catalog.sources(actor, &recording.id).await.unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].id == sources[0].id && after[0].ready);
}
