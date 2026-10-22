use hito_core::test_support::podcast_audio_service as fixture;
use hito_core::test_support::support;

use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AudioRequest, CoreConfig, Credentials, DownloadRequest, DownloadState,
    Downloads, Error, HostDeviceId, OpenedAudio, PodcastEpisodeQuery, PodcastNetworkConfig,
    Services, SubscribePodcast, UnsubscribePodcast, open,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn ready_podcast_download_remains_authorized_after_unsubscribe_and_upstream_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-retained-upstream.wav"), 173);
    let mut server = PodcastAudioServer::start(wav.clone()).await;
    let mut downloads = None;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(
            directory.path(),
            &mut server,
            &wav,
            &mut downloads,
        ))
        .catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    let settled = match downloads {
        Some(downloads) => matches!(
            tokio::time::timeout(Duration::from_secs(3), downloads.finish_downloads()).await,
            Ok(Ok(()))
        ),
        None => true,
    };
    assert!(
        joined && settled,
        "the owned service and managed storage must join on success, panic or timeout"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the ready podcast retention exercise must finish within its deadline"),
    }
}

async fn exercise(
    directory: &Path,
    server: &mut PodcastAudioServer,
    wav: &[u8],
    cleanup: &mut Option<Downloads>,
) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-download-retention").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    *cleanup = Some(hito.downloads.clone());
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-download-retention-owner".into(),
            password: "owned ready podcast retention password".into(),
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
    let page = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    let recording = Uuid::parse_str(&page.items[0].recording.id).unwrap();
    let recording_id = recording.to_string();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "My retained unsubscribed episode".into(),
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
        "My retained unsubscribed episode"
    );
    assert_eq!(before["recordings"][0]["liked"], true);
    assert_eq!(before["recordings"][0]["available"], true);
    assert_eq!(
        before["progress"],
        json!([
            { "recording_id": recording, "position_ms": 700, "speed": 1.25 }
        ])
    );
    let remote = hito
        .catalog
        .preferred_playback_source(&actor, &recording_id)
        .await
        .unwrap();
    assert!(!remote.is_local());
    assert!(matches!(
        hito.downloads.status(&actor, &recording_id).await,
        Err(Error::NotFound)
    ));
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 0);

    let request_id = Uuid::new_v4();
    let accepted = hito
        .downloads
        .create(&actor, &recording_id, DownloadRequest { request_id })
        .await
        .unwrap();
    assert_eq!(accepted.request_id, request_id);
    assert_eq!(accepted.recording_id, recording_id);
    assert_eq!(server.enclosure_requests(), 0);
    assert!(hito.downloads.run_next_job_until(pending()).await.unwrap());
    let ready = hito.downloads.status(&actor, &recording_id).await.unwrap();
    assert!(matches!(ready.status, DownloadState::Ready));
    assert_eq!(ready.intent_id, accepted.intent_id);
    assert_eq!(ready.recording_id, recording_id);
    assert_eq!(ready.bytes_received, wav.len() as u64);
    assert_eq!(ready.total_bytes, Some(wav.len() as u64));
    assert!(ready.error.is_none() && ready.matches_source_choice);
    let saved_source = ready
        .source_id
        .as_deref()
        .expect("the completed download must publish its managed source");
    assert_ne!(saved_source, remote.source_id());
    let ready_snapshot = serde_json::to_value(&ready).unwrap();
    let saved = hito
        .catalog
        .preferred_playback_source(&actor, &recording_id)
        .await
        .unwrap();
    assert!(saved.is_local());
    assert_eq!(saved.source_id(), saved_source);
    saved.check().await.unwrap();
    assert_eq!(personal_state(&hito, &actor, recording).await, before);
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
    assert!(matches!(remote.check().await, Err(Error::NotFound)));
    saved.check().await.unwrap();
    assert_eq!(
        serde_json::to_value(hito.downloads.status(&actor, &recording_id).await.unwrap()).unwrap(),
        ready_snapshot
    );
    let sources = hito.catalog.sources(&actor, &recording_id).await.unwrap();
    assert_eq!(sources.len(), 2);
    let retained_remote = sources
        .iter()
        .find(|source| source.id == remote.source_id())
        .unwrap();
    assert_eq!(retained_remote.provider, "podcast");
    assert!(!retained_remote.ready);
    let retained_saved = sources
        .iter()
        .find(|source| source.id == saved_source)
        .unwrap();
    assert_eq!(retained_saved.provider, "download");
    assert!(retained_saved.ready);
    assert_eq!(
        retained_saved.device_id.as_deref(),
        Some("podcast-download-retention")
    );
    assert_eq!(personal_state(&hito, &actor, recording).await, before);
    assert_requests(server);

    assert!(
        server.finish().await,
        "the real upstream listener must stop before local playback"
    );
    let requests_at_shutdown = server.requests();
    let preferred = hito
        .catalog
        .preferred_playback_source(&actor, &recording_id)
        .await
        .unwrap();
    assert!(preferred.is_local());
    assert_eq!(preferred.source_id(), saved_source);
    saved.check().await.unwrap();
    let OpenedAudio::Content(mut audio) = saved.open(AudioRequest::default()).await.unwrap() else {
        panic!("the retained managed grant must return complete WAV audio after unsubscribe");
    };
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(wav.len() as u64));
    assert_eq!(audio.total_length, Some(wav.len() as u64));
    assert!(audio.range.is_none());
    let body = audio
        .body
        .as_mut()
        .expect("retained local playback must expose its WAV body");
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= 65536 && chunk.len() <= wav.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received.as_slice(), wav);
    assert_eq!(server.requests(), requests_at_shutdown);

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
    assert!(
        hito.podcasts
            .subscriptions(&actor)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(hito.downloads.status(&actor, &recording_id).await.unwrap()).unwrap(),
        ready_snapshot
    );
    assert_eq!(personal_state(&hito, &actor, recording).await, before);
    hito.identity.check_session(&actor).await.unwrap();
    assert_eq!(actor.user().id, session.user.id);
    let public_facts =
        json!({ "personal": before, "sources": sources, "download": ready_snapshot }).to_string();
    assert!(!public_facts.contains(&server.enclosure_url));
    assert_eq!(
        std::fs::read(directory.join("owned-retained-upstream.wav"))
            .unwrap()
            .as_slice(),
        wav
    );
    assert_eq!(server.requests(), requests_at_shutdown);
    assert_requests(server);
}

async fn personal_state(hito: &Services, actor: &Actor, recording: Uuid) -> Value {
    json!({
        "recordings": hito.catalog.recordings_by_ids(actor, &[recording]).await.unwrap(),
        "progress": hito.collection.saved_playback(actor, &[recording]).await.unwrap(),
    })
}

fn assert_requests(server: &PodcastAudioServer) {
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 1);
    assert_eq!(server.requests(), 2);
    assert_eq!(server.unexpected(), 0);
}
