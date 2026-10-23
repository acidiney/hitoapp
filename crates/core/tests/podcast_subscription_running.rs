use hito_core::test_support::podcast_audio_service as audio_fixture;
#[path = "fixtures/podcast_held_feed.rs"]
mod feed_fixture;
use hito_core::test_support::support;

use audio_fixture::PodcastAudioServer;
use feed_fixture::HeldFeedServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AudioRequest, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PodcastEpisodeQuery, PodcastNetworkConfig, RefreshPodcast, Services, SubscribePodcast,
    UnsubscribePodcast, open,
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

struct Episode {
    subscription_id: Uuid,
    episode_id: Uuid,
    recording_id: Uuid,
    source_id: String,
    playlist_id: Uuid,
    entries: [Uuid; 2],
}

#[tokio::test]
async fn reactivation_waits_for_the_obsolete_running_worker_and_preserves_private_state() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-running-worker.wav"), 53);
    let mut audio = PodcastAudioServer::start(wav.clone()).await;
    let mut feed = HeldFeedServer::start(&audio.enclosure_url, wav.len()).await;
    assert!(
        feed.feed_url != audio.feed_url,
        "the held feed and WAV enclosure must use separate owned services"
    );
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &mut feed, &audio, &wav)).catch_unwind(),
    )
    .await;
    let feed_joined = feed.finish().await;
    let audio_joined = audio.finish().await;
    assert!(
        feed_joined && audio_joined,
        "both owned services must shut down and join on every outcome"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the running-worker lifecycle must finish within its deadline"),
    }
}

async fn exercise(
    directory: &Path,
    feed: &mut HeldFeedServer,
    audio: &PodcastAudioServer,
    wav: &[u8],
) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-running-worker").unwrap(),
    );
    config.podcast_network =
        PodcastNetworkConfig::new(vec![feed.origin.clone(), audio.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-running-owner".into(),
            password: "owned running podcast password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let initial = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: Uuid::new_v4(),
                feed_url: feed.feed_url.clone(),
            },
        )
        .await
        .unwrap();
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let mut page = hito
        .podcasts
        .episodes(
            &actor,
            &initial.subscription_id.to_string(),
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    let episode = page.items.remove(0);
    let old_grant = hito
        .playback
        .preferred_playback_source(&actor, &episode.recording.id)
        .await
        .unwrap();
    old_grant.check().await.unwrap();
    let retained = Episode {
        subscription_id: initial.subscription_id,
        episode_id: episode.episode_id,
        recording_id: Uuid::parse_str(&episode.recording.id).unwrap(),
        source_id: old_grant.source_id().to_owned(),
        playlist_id: Uuid::new_v4(),
        entries: [Uuid::new_v4(), Uuid::new_v4()],
    };
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: retained.recording_id,
                title: "My running podcast episode".into(),
            },
            Action::LikeSet {
                recording_id: retained.recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: retained.recording_id,
                position_ms: 700,
                speed: 1.5,
            },
            Action::PlaylistCreate {
                playlist_id: retained.playlist_id,
                name: "My running podcast playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: retained.playlist_id,
                entry_id: retained.entries[0],
                recording_id: retained.recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id: retained.playlist_id,
                entry_id: retained.entries[1],
                recording_id: retained.recording_id,
                position: 1,
            },
        ],
    )
    .await;
    assert_private_state(&hito, &actor, &retained).await;
    assert_requests(feed, audio, 1, 0);
    let refresh = hito
        .podcasts
        .enqueue_refresh(
            &actor,
            &retained.subscription_id.to_string(),
            RefreshPodcast {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_ne!(refresh.job_id, initial.job_id);
    let worker_services = hito.clone();
    let mut worker = Box::pin(async move {
        worker_services
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
    });
    tokio::select! {
        result = &mut worker => panic!("the real refresh returned before its owned response gate: {}", result.is_ok()),
        () = feed.wait_for_held_request() => {},
    }
    assert_requests(feed, audio, 2, 0);
    assert_eq!(feed.held_finished(), 0);

    let removed = hito
        .podcasts
        .unsubscribe(
            &actor,
            &retained.subscription_id.to_string(),
            UnsubscribePodcast {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_eq!(removed.subscription_id, retained.subscription_id);
    let reactivation_id = Uuid::new_v4();
    let reactivation = || SubscribePodcast {
        request_id: reactivation_id,
        feed_url: feed.feed_url.clone(),
    };
    assert!(
        matches!(
            hito.podcasts.subscribe(&actor, reactivation()).await,
            Err(Error::Conflict(_))
        ),
        "an actual pending old worker must retain its admission and prevent reactivation"
    );
    assert_inactive(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    assert_requests(feed, audio, 2, 0);
    assert_eq!(feed.held_finished(), 0);

    assert!(
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .expect("the resumed obsolete worker must retire through its authority heartbeat")
            .unwrap()
    );
    assert_eq!(
        feed.held_finished(),
        0,
        "the old worker must retire before the owned server releases its held response"
    );
    assert_inactive(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    let retried = hito
        .podcasts
        .subscribe(&actor, reactivation())
        .await
        .unwrap();
    assert_eq!(retried.request_id, reactivation_id);
    assert_eq!(retried.subscription_id, retained.subscription_id);
    assert_ne!(retried.job_id, initial.job_id);
    assert_ne!(retried.job_id, refresh.job_id);
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let page = hito
        .podcasts
        .episodes(
            &actor,
            &retained.subscription_id.to_string(),
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].episode_id, retained.episode_id);
    assert_eq!(
        page.items[0].recording.id,
        retained.recording_id.to_string()
    );
    assert_private_state(&hito, &actor, &retained).await;
    let new_grant = hito
        .playback
        .preferred_playback_source(&actor, &retained.recording_id.to_string())
        .await
        .unwrap();
    assert_eq!(new_grant.source_id(), retained.source_id);
    new_grant.check().await.unwrap();
    assert!(
        matches!(old_grant.check().await, Err(Error::NotFound)),
        "a grant first rechecked after reactivation must still reject its obsolete generation"
    );
    assert!(matches!(
        old_grant.open(AudioRequest::default()).await,
        Err(Error::NotFound)
    ));
    assert_requests(feed, audio, 3, 0);
    assert_eq!(feed.held_finished(), 0);
    let OpenedAudio::Content(mut content) = new_grant.open(AudioRequest::default()).await.unwrap()
    else {
        panic!("a fresh reactivated grant must open the owned WAV");
    };
    let body = content.body.as_mut().unwrap();
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= wav.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, wav);
    assert_private_state(&hito, &actor, &retained).await;
    assert_requests(feed, audio, 3, 1);
}

async fn assert_inactive(hito: &Services, actor: &Actor, retained: &Episode) {
    assert!(hito.podcasts.subscriptions(actor).await.unwrap().is_empty());
    assert!(matches!(
        hito.podcasts
            .episodes(
                actor,
                &retained.subscription_id.to_string(),
                PodcastEpisodeQuery::default(),
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.playback
            .preferred_playback_source(actor, &retained.recording_id.to_string(),)
            .await,
        Err(Error::NotFound)
    ));
}

async fn assert_private_state(hito: &Services, actor: &Actor, retained: &Episode) {
    let recordings = hito
        .catalog
        .recordings_by_ids(actor, &[retained.recording_id])
        .await
        .unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].title, "My running podcast episode");
    assert!(recordings[0].liked);
    assert_eq!(
        hito.collection
            .saved_position(actor, retained.recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        700
    );
    let history = hito.collection.activity(actor).await.unwrap().history;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, retained.recording_id.to_string());
    assert_eq!(history[0].speed, 1.5);
    let playlist = hito
        .collection
        .playlist(actor, &retained.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 2);
    let recording = retained.recording_id.to_string();
    for (index, entry) in playlist.entries.iter().enumerate() {
        assert_eq!(entry.entry_id, retained.entries[index].to_string());
        assert_eq!(entry.recording_id.as_deref(), Some(recording.as_str()));
        assert_eq!(entry.position, index as i64);
        assert_eq!(entry.title, "My running podcast episode");
        assert!(entry.liked);
    }
    assert!(matches!(
        hito.downloads.status(actor, &recording).await,
        Err(Error::NotFound)
    ));
}

fn assert_requests(
    feed: &HeldFeedServer,
    audio: &PodcastAudioServer,
    feeds: usize,
    enclosures: usize,
) {
    assert_eq!(feed.requests(), feeds);
    assert_eq!(feed.unexpected(), 0);
    assert_eq!(audio.requests(), enclosures);
    assert_eq!(audio.feed_requests(), 0);
    assert_eq!(audio.enclosure_requests(), enclosures);
    assert_eq!(audio.unexpected(), 0);
}
