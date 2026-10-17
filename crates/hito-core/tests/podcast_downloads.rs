#[path = "fixtures/podcast_audio_service.rs"]
mod fixture;
mod support;

use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AudioRequest, CoreConfig, Credentials, DownloadRequest, DownloadState, Error,
    HostDeviceId, OpenedAudio, PodcastEpisodeQuery, PodcastNetworkConfig, Services,
    SubscribePodcast, open,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn explicit_podcast_download_survives_logout_restart_and_upstream_outage() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-download.wav"), 149);
    let mut server = PodcastAudioServer::start(wav.clone()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(directory.path(), &mut server, &wav)).catch_unwind(),
    )
    .await;
    assert!(
        server.finish().await,
        "the owned RSS and WAV service must join on success, panic or timeout"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the explicit podcast download must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &mut PodcastAudioServer, wav: &[u8]) {
    let hito = open(config(directory, &server.origin)).await.unwrap();
    let session = hito.identity.setup(credentials()).await.unwrap();
    let requesting = hito.identity.authenticate(&session.token).await.unwrap();
    let subscription = hito
        .podcasts
        .subscribe(
            &requesting,
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
    let episodes = hito
        .podcasts
        .episodes(
            &requesting,
            &subscription.subscription_id.to_string(),
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(episodes.total, 1);
    assert_eq!(episodes.items.len(), 1);
    let recording = Uuid::parse_str(&episodes.items[0].recording.id).unwrap();
    let recording_id = recording.to_string();
    support::synchronize(
        &hito,
        &requesting,
        vec![
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "My retained downloaded episode".into(),
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
    let before = personal_state(&hito, &requesting, subscription.subscription_id, recording).await;
    assert_eq!(
        before["recordings"][0]["title"],
        "My retained downloaded episode"
    );
    assert_eq!(before["recordings"][0]["liked"], true);
    assert_eq!(
        before["progress"],
        json!([
            { "recording_id": recording, "position_ms": 700, "speed": 1.25 }
        ])
    );
    assert!(matches!(
        hito.downloads.status(&requesting, &recording_id).await,
        Err(Error::NotFound)
    ));
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 0);
    assert_eq!(server.unexpected(), 0);

    let request_id = Uuid::new_v4();
    let accepted = hito
        .downloads
        .create(&requesting, &recording_id, DownloadRequest { request_id })
        .await
        .expect("an explicit RSS download must admit the published podcast source");
    assert_eq!(accepted.request_id, request_id);
    assert_eq!(accepted.recording_id, recording_id);
    let queued = hito
        .downloads
        .status(&requesting, &recording_id)
        .await
        .unwrap();
    assert!(matches!(queued.status, DownloadState::Queued));
    assert_eq!(queued.intent_id, accepted.intent_id);
    assert_eq!(queued.bytes_received, 0);
    assert!(queued.source_id.is_none());
    let public_facts = json!({
        "personal": &before,
        "receipt": &accepted,
        "download": &queued,
        "sources": hito.catalog.sources(&requesting, &recording_id).await.unwrap(),
    })
    .to_string();
    assert!(
        !public_facts.contains(&server.enclosure_url),
        "public download and source facts must omit the captured enclosure address"
    );
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 0);

    hito.identity.logout(&session.token).await.unwrap();
    assert!(matches!(
        hito.identity.check_session(&requesting).await,
        Err(Error::Unauthorized)
    ));
    drop(requesting);
    assert!(hito.downloads.run_next_job_until(pending()).await.unwrap());
    let observing = hito.identity.login(credentials()).await.unwrap();
    let actor = hito.identity.authenticate(&observing.token).await.unwrap();
    assert_eq!(actor.user().id, session.user.id);
    let ready = hito.downloads.status(&actor, &recording_id).await.unwrap();
    assert!(
        matches!(ready.status, DownloadState::Ready),
        "state={:?}, error={:?}",
        ready.status,
        ready.error
    );
    assert_eq!(ready.intent_id, accepted.intent_id);
    assert_eq!(ready.bytes_received, wav.len() as u64);
    assert_eq!(ready.total_bytes, Some(wav.len() as u64));
    assert_eq!(ready.host_device_id, "podcast-downloads");
    let downloaded_source = ready.source_id.unwrap();
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 1);
    assert_eq!(server.unexpected(), 0);
    assert_eq!(
        personal_state(&hito, &actor, subscription.subscription_id, recording).await,
        before
    );
    hito.downloads.finish_downloads().await.unwrap();
    drop(actor);
    drop(hito);

    assert!(
        server.finish().await,
        "the upstream must be unavailable before reopening Hito"
    );
    let requests_at_outage = server.requests();
    let reopened = open(config(directory, &server.origin)).await.unwrap();
    let session = reopened.identity.login(credentials()).await.unwrap();
    let actor = reopened
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let retained = reopened
        .downloads
        .status(&actor, &recording_id)
        .await
        .unwrap();
    assert!(matches!(retained.status, DownloadState::Ready));
    assert_eq!(retained.intent_id, accepted.intent_id);
    assert_eq!(
        retained.source_id.as_deref(),
        Some(downloaded_source.as_str())
    );
    let preferred = reopened
        .catalog
        .preferred_playback_source(&actor, &recording_id)
        .await
        .unwrap();
    assert!(preferred.is_local());
    assert_eq!(preferred.source_id(), downloaded_source);
    let OpenedAudio::Content(mut audio) = reopened
        .catalog
        .open_audio(&actor, preferred.source_id(), AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("the retained podcast download must return full audio");
    };
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(wav.len() as u64));
    assert!(audio.range.is_none());
    let mut received = Vec::new();
    let body = audio.body.as_mut().expect("the saved WAV must have a body");
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= wav.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received.as_slice(), wav);
    let replay = reopened
        .downloads
        .create(&actor, &recording_id, DownloadRequest { request_id })
        .await
        .unwrap();
    assert_eq!(replay.request_id, accepted.request_id);
    assert_eq!(replay.intent_id, accepted.intent_id);
    assert_eq!(replay.recording_id, accepted.recording_id);
    assert!(
        !reopened
            .downloads
            .run_next_job_until(pending())
            .await
            .unwrap()
    );
    assert_eq!(
        personal_state(&reopened, &actor, subscription.subscription_id, recording).await,
        before
    );
    assert_eq!(server.requests(), requests_at_outage);
    reopened.downloads.finish_downloads().await.unwrap();
}

fn credentials() -> Credentials {
    Credentials {
        username: "podcast-download-owner".into(),
        password: "owned podcast download password".into(),
    }
}

fn config(directory: &Path, origin: &str) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-downloads").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![origin.into()]).unwrap();
    config
}

async fn personal_state(
    hito: &Services,
    actor: &Actor,
    subscription: Uuid,
    recording: Uuid,
) -> Value {
    json!({
        "episodes": hito.podcasts.episodes(actor, &subscription.to_string(), PodcastEpisodeQuery::default()).await.unwrap(),
        "recordings": hito.catalog.recordings_by_ids(actor, &[recording]).await.unwrap(),
        "progress": hito.collection.saved_playback(actor, &[recording]).await.unwrap(),
    })
}
