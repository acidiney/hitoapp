#[path = "fixtures/podcast_audio_service.rs"]
mod fixture;
mod support;

use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, CoreConfig, Credentials, DownloadRequest, DownloadState, Error, HostDeviceId,
    PodcastEpisodeQuery, PodcastNetworkConfig, Services, SubscribePodcast, UnsubscribePodcast,
    open,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn unsubscribe_revokes_queued_podcast_download_without_receipt_reactivation() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-upstream.wav"), 157);
    let mut server = PodcastAudioServer::start(wav.clone()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &server, &wav)).catch_unwind(),
    )
    .await;
    assert!(
        server.finish().await,
        "the owned RSS and WAV service must join on success, panic or timeout"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the queued podcast revocation must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &PodcastAudioServer, wav: &[u8]) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-download-revocation").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-download-revocation-owner".into(),
            password: "owned queued podcast revocation password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let subscribed = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: Uuid::new_v4(),
                feed_url: server.feed_url.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(server.requests(), 0);
    assert!(
        hito.podcasts
            .run_next_refresh_until(pending())
            .await
            .unwrap()
    );
    let subscription = subscribed.subscription_id.to_string();
    let episodes = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(episodes.total, 1);
    assert_eq!(episodes.items.len(), 1);
    let recording = Uuid::parse_str(&episodes.items[0].recording.id).unwrap();
    let recording_id = recording.to_string();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "My retained revoked-download episode".into(),
            },
            Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 700,
                speed: 1.25,
            },
        ],
    )
    .await;
    let before = personal_state(&hito, &actor, recording).await;
    assert_eq!(before["recordings"][0]["id"], recording_id);
    assert_eq!(
        before["recordings"][0]["title"],
        "My retained revoked-download episode"
    );
    assert_eq!(before["recordings"][0]["liked"], true);
    assert_eq!(
        before["progress"],
        json!([
            { "recording_id": recording, "position_ms": 700, "speed": 1.25 }
        ])
    );
    let original_sources = hito.catalog.sources(&actor, &recording_id).await.unwrap();
    assert_eq!(original_sources.len(), 1);
    assert_eq!(original_sources[0].provider, "podcast");
    assert!(original_sources[0].ready);
    let original_source = &original_sources[0].id;

    let request_id = Uuid::new_v4();
    let accepted = hito
        .downloads
        .create(&actor, &recording_id, DownloadRequest { request_id })
        .await
        .unwrap();
    assert_eq!(accepted.request_id, request_id);
    assert_eq!(accepted.recording_id, recording_id);
    let queued = hito.downloads.status(&actor, &recording_id).await.unwrap();
    assert!(matches!(queued.status, DownloadState::Queued));
    assert_eq!(queued.intent_id, accepted.intent_id);
    assert_eq!(queued.bytes_received, 0);
    assert!(queued.source_id.is_none());
    assert_requests(server);

    let unsubscribe_id = Uuid::new_v4();
    let removed = hito
        .podcasts
        .unsubscribe(
            &actor,
            &subscription,
            UnsubscribePodcast {
                request_id: unsubscribe_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(removed.request_id, unsubscribe_id);
    assert_eq!(removed.subscription_id, subscribed.subscription_id);
    assert!(
        hito.podcasts
            .subscriptions(&actor)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        hito.podcasts
            .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
            .await,
        Err(Error::NotFound)
    ));

    hito.downloads.run_next_job_until(pending()).await.unwrap();
    let failed = hito.downloads.status(&actor, &recording_id).await.unwrap();
    assert!(matches!(failed.status, DownloadState::Failed));
    assert_eq!(failed.error.as_deref(), Some("authority_changed"));
    assert_eq!(failed.intent_id, accepted.intent_id);
    assert_eq!(failed.recording_id, recording_id);
    assert_eq!(failed.bytes_received, 0);
    assert!(failed.source_id.is_none() && failed.total_bytes.is_none());
    let terminal = serde_json::to_value(&failed).unwrap();
    assert_requests(server);

    let replay = hito
        .downloads
        .create(&actor, &recording_id, DownloadRequest { request_id })
        .await
        .unwrap();
    assert_eq!(replay.request_id, accepted.request_id);
    assert_eq!(replay.intent_id, accepted.intent_id);
    assert_eq!(replay.recording_id, accepted.recording_id);
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(pending())
            .await
            .unwrap()
    );
    assert_eq!(
        serde_json::to_value(hito.downloads.status(&actor, &recording_id).await.unwrap()).unwrap(),
        terminal
    );
    assert!(
        hito.podcasts
            .subscriptions(&actor)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(personal_state(&hito, &actor, recording).await, before);
    hito.identity.check_session(&actor).await.unwrap();
    assert_eq!(actor.user().id, session.user.id);

    let sources = hito.catalog.sources(&actor, &recording_id).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(&sources[0].id, original_source);
    assert_eq!(sources[0].provider, "podcast");
    assert!(!sources[0].ready);
    assert!(matches!(
        hito.catalog
            .preferred_playback_source(&actor, &recording_id)
            .await,
        Err(Error::NotFound)
    ));
    let public_facts =
        json!({ "recording": before, "sources": sources, "download": terminal }).to_string();
    assert!(
        !public_facts.contains(&server.enclosure_url),
        "retained public facts must omit the revoked enclosure address"
    );
    assert_eq!(
        std::fs::read(directory.join("owned-upstream.wav"))
            .unwrap()
            .as_slice(),
        wav
    );
    assert_requests(server);
    hito.downloads.finish_downloads().await.unwrap();
}

async fn personal_state(hito: &Services, actor: &Actor, recording: Uuid) -> Value {
    let recordings = hito
        .catalog
        .recordings_by_ids(actor, &[recording])
        .await
        .unwrap();
    json!({
        "recordings": recordings.iter().map(|recording| json!({
            "id": recording.id,
            "library_id": recording.library_id,
            "release_id": recording.release_id,
            "title": recording.title,
            "inherited_title": recording.inherited_title,
            "inherited_artist": recording.inherited_artist,
            "artist": recording.artist,
            "album": recording.album,
            "duration_ms": recording.duration_ms,
            "liked": recording.liked,
        })).collect::<Vec<_>>(),
        "progress": hito.collection.saved_playback(actor, &[recording]).await.unwrap(),
    })
}

fn assert_requests(server: &PodcastAudioServer) {
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 0);
    assert_eq!(server.requests(), 1);
    assert_eq!(server.unexpected(), 0);
}
