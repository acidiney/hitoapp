#![cfg(unix)]

#[path = "fixtures/community_service.rs"]
mod community;
#[path = "fixtures/content_service.rs"]
mod provider;
mod support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, CommunityConfig, CommunityReportAdmission, CoreConfig,
    Credentials, HostDeviceId, InstallPlugin, OfferPluginService, OpenedAudio,
    PreviewPluginInstallation, Services, SuccessfulPlayback, UpdateCommunityPreferences,
};
use serde_json::{Value, json};
use std::{future::pending, io::Write, os::unix::fs::OpenOptionsExt, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn reporting_requires_playback_consent_and_queues_only_redacted_public_source_claims() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-originals-canary");
    std::fs::create_dir(&music).unwrap();
    let original_path = music.join("private-audio-canary.wav");
    let original = support::write_wav(&original_path, 27);
    let remote = community::CommunityServer::start().await;
    let token_path = directory.path().join("private-community-admin-canary");
    let mut token_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&token_path)
        .unwrap();
    token_file
        .write_all(community::ADMIN_TOKEN.as_bytes())
        .unwrap();
    drop(token_file);
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music.clone(),
        HostDeviceId::new("private-community-host-canary").unwrap(),
    );
    config.community = Some(CommunityConfig::new(remote.url.clone(), token_path.clone()).unwrap());
    let hito = hito_core::open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "private-listener-canary".into(),
            password: "private-session-password-canary".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let provider = provider::ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.jamendo", "name": "Owned public-source fixture",
                "protocol_version": 1, "capabilities": ["search", "sources"],
                "content_policy": {"source_persistence": "playback_only"}
            }),
        )
        .await;
    provider
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "jamendo:track:v1:42", "title": "private-title-canary",
                "artist": "private-artist-canary", "duration_ms": 90_000,
                "release": {"id": "jamendo:album:v1:7", "title": "private-album-canary"}
            }),
        )
        .await;
    provider.reply("v1/sources", json!({"items": [{
        "id": "jamendo:audio:v1:42:mp32", "label": "private-source-label-canary", "ready": true
    }]})).await;
    provider
        .audio(
            axum::http::StatusCode::OK,
            axum::http::HeaderMap::from_iter([(
                "content-type".parse().unwrap(),
                "audio/wav".parse().unwrap(),
            )]),
            original.clone(),
        )
        .await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned official provider identity".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let preview = hito
        .plugins
        .preview_installation(
            &actor,
            PreviewPluginInstallation {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: Some("private-provider-credential-canary".into()),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                accept_manifest_digest: Some(preview.digest),
                bearer_token: Some("private-provider-credential-canary".into()),
            },
        )
        .await
        .unwrap();
    let recording = hito
        .catalog
        .adopt_provider(
            &actor,
            &installation.id,
            AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "jamendo:track:v1:42".into(),
            },
        )
        .await
        .unwrap();
    let source = hito
        .catalog
        .sources(&actor, &recording.id)
        .await
        .unwrap()
        .remove(0);
    assert!(source.ready);
    let playlist = Uuid::new_v4();
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "private-playlist-canary".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::RecordingTitleSet {
                recording_id,
                title: "private-personal-title-canary".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 9_000,
                speed: 1.25,
            },
        ],
    )
    .await;
    let before = private_state(&hito, &actor, playlist, &recording.id).await;
    let submission_id = Uuid::new_v4();
    let preferences = hito.community.preferences(&actor).await.unwrap();
    assert!(!preferences.playback_reporting && !preferences.metadata_corrections);
    assert_eq!(
        hito.community
            .observe_success(&actor, &source.id, SuccessfulPlayback { submission_id })
            .await
            .unwrap(),
        CommunityReportAdmission::Ignored
    );
    assert!(!run_worker(&hito).await);
    assert!(remote.requests().await.is_empty());

    let preferences = hito
        .community
        .update_preferences(
            &actor,
            UpdateCommunityPreferences {
                revision: preferences.revision,
                playback_reporting: false,
                metadata_corrections: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        hito.community.preferences(&actor).await.unwrap(),
        preferences
    );
    assert_eq!(
        hito.community
            .observe_success(&actor, &source.id, SuccessfulPlayback { submission_id })
            .await
            .unwrap(),
        CommunityReportAdmission::Ignored
    );
    assert!(!run_worker(&hito).await);
    assert!(remote.requests().await.is_empty());

    hito.community
        .update_preferences(
            &actor,
            UpdateCommunityPreferences {
                revision: preferences.revision,
                playback_reporting: true,
                metadata_corrections: true,
            },
        )
        .await
        .unwrap();
    assert!(
        remote.requests().await.is_empty(),
        "Saving consent must not provision a reporter or send a report"
    );
    let grant = hito
        .catalog
        .playback_source(&actor, &recording.id, &source.id)
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = grant.open(Default::default()).await.unwrap() else {
        panic!("The owned deterministic provider audio must open");
    };
    let mut received = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, original);
    assert_eq!(
        hito.community
            .observe_success(&actor, &source.id, SuccessfulPlayback { submission_id })
            .await
            .unwrap(),
        CommunityReportAdmission::Queued
    );
    assert!(
        remote.requests().await.is_empty(),
        "Success admission must only queue locally"
    );
    assert!(run_worker(&hito).await);
    let requests = remote.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/v1/admin/reporters");
    assert_eq!(requests[0].body, json!({}));
    assert!(
        requests[0].authorization.as_deref()
            == Some(format!("Bearer {}", community::ADMIN_TOKEN).as_str())
    );
    assert_eq!(requests[1].path, "/v1/outcomes");
    assert!(
        requests[1].authorization.as_deref()
            == Some(format!("Bearer {}", community::REPORTER_TOKEN).as_str())
    );
    assert_eq!(
        requests[1].body,
        json!({
            "submission_id": submission_id,
            "source": {"provider": "jamendo", "recording_id": "42"},
            "outcome": "successful_playback"
        })
    );
    let contributed = serde_json::to_string(
        &requests
            .iter()
            .map(|request| &request.body)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for forbidden in [
        actor.user().id.as_str(),
        session.token.as_str(),
        source.id.as_str(),
        recording.id.as_str(),
        installation.id.as_str(),
        provider.url.as_str(),
        community::ADMIN_TOKEN,
        community::REPORTER_TOKEN,
        "private-",
        "jamendo:track:",
        "jamendo:audio:",
        "http",
        "RIFF",
    ] {
        assert!(
            !contributed.contains(forbidden),
            "Contributions must contain only the accepted public source claim"
        );
    }
    assert!(!run_worker(&hito).await);
    assert_eq!(remote.requests().await.len(), 2);
    assert_eq!(
        private_state(&hito, &actor, playlist, &recording.id).await,
        before
    );
    assert_eq!(std::fs::read(original_path).unwrap(), original);
    assert!(std::fs::read(token_path).unwrap() == community::ADMIN_TOKEN.as_bytes());
}

async fn run_worker(hito: &Services) -> bool {
    tokio::time::timeout(
        Duration::from_secs(5),
        hito.community.run_next_report_until(pending()),
    )
    .await
    .unwrap()
    .unwrap()
}

async fn private_state(hito: &Services, actor: &Actor, playlist: Uuid, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "libraries": hito.catalog.libraries(actor).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "plugins": hito.plugins.settings(actor).await.unwrap(),
        "playlist": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "progress": hito.collection.saved_position(actor, Uuid::parse_str(recording).unwrap()).await.unwrap().map(|saved| saved.position_ms),
    })
}
