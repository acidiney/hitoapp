use hito_core::test_support::podcast_audio_service as fixture;
use hito_core::test_support::support;

use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AudioRequest, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PlaybackSource, PodcastEpisodeQuery, PodcastNetworkConfig, PodcastUnsubscribeReceipt,
    RefreshPodcast, Services, SubscribePodcast, UnsubscribePodcast, open,
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

const PRIVATE_TITLE: &str = "My retained podcast episode";

struct RetainedEpisode {
    subscription_id: Uuid,
    episode_id: Uuid,
    recording_id: Uuid,
    source_id: Uuid,
    playlist_id: Uuid,
    entries: [Uuid; 2],
}

#[tokio::test]
async fn unsubscribe_and_resubscribe_preserve_identity_private_state_and_immutable_receipts() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let wav = support::write_wav(&directory.path().join("owned-lifecycle.wav"), 137);
    let mut server = PodcastAudioServer::start(wav.clone()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(exercise(directory.path(), &server, &wav)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned service must shut down and join after the lifecycle exercise"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public subscription lifecycle must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &PodcastAudioServer, wav: &[u8]) {
    let hito = open(config(directory, server)).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-lifecycle-owner".into(),
            password: "owned podcast lifecycle password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let subscribe_id = Uuid::new_v4();
    let original_request = || SubscribePodcast {
        request_id: subscribe_id,
        feed_url: server.feed_url.clone(),
    };
    let subscribed = hito
        .podcasts
        .subscribe(&actor, original_request())
        .await
        .unwrap();
    assert_eq!(subscribed.request_id, subscribe_id);
    assert_requests(server, 0, 0);
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_requests(server, 1, 0);

    let mut episodes = hito
        .podcasts
        .episodes(
            &actor,
            &subscribed.subscription_id.to_string(),
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(episodes.total, 1);
    assert_eq!(episodes.items.len(), 1);
    let episode = episodes.items.remove(0);
    let initial_audio = hito
        .playback
        .preferred_playback_source(&actor, &episode.recording.id)
        .await
        .unwrap();
    let retained = RetainedEpisode {
        subscription_id: subscribed.subscription_id,
        episode_id: episode.episode_id,
        recording_id: Uuid::parse_str(&episode.recording.id).unwrap(),
        source_id: Uuid::parse_str(initial_audio.source_id()).unwrap(),
        playlist_id: Uuid::new_v4(),
        entries: [Uuid::new_v4(), Uuid::new_v4()],
    };
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: retained.recording_id,
                title: PRIVATE_TITLE.into(),
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
                name: "My retained podcast playlist".into(),
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
    assert_active(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    let public_sources = hito
        .catalog
        .sources(&actor, &retained.recording_id.to_string())
        .await
        .unwrap();
    assert!(
        !serde_json::to_string(&public_sources)
            .unwrap()
            .contains(&server.enclosure_url),
        "public source facts must not expose the retained enclosure address"
    );
    assert_requests(server, 1, 0);
    assert_wav(&initial_audio, wav).await;
    assert_requests(server, 1, 1);
    drop(initial_audio);

    server.publish_enclosure(wav.to_vec()).await;
    let refreshed = hito
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
    assert_ne!(refreshed.job_id, subscribed.job_id);
    assert_requests(server, 1, 1);
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_active(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    assert_requests(server, 2, 1);
    let captured = hito
        .playback
        .preferred_playback_source(&actor, &retained.recording_id.to_string())
        .await
        .unwrap();
    assert_eq!(captured.source_id(), retained.source_id.to_string());
    captured.check().await.unwrap();
    let queued = hito
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
    assert_ne!(queued.job_id, refreshed.job_id);
    let unsubscribe_id = Uuid::new_v4();
    let removed: PodcastUnsubscribeReceipt = hito
        .podcasts
        .unsubscribe(
            &actor,
            &retained.subscription_id.to_string(),
            UnsubscribePodcast {
                request_id: unsubscribe_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(removed.request_id, unsubscribe_id);
    assert_eq!(removed.subscription_id, retained.subscription_id);
    assert_inactive(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    assert!(matches!(captured.check().await, Err(Error::NotFound)));
    assert!(matches!(
        captured.open(AudioRequest::default()).await,
        Err(Error::NotFound)
    ));
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "unsubscribe must cancel queued refresh work rather than fetch the feed"
    );
    assert_requests(server, 2, 1);
    drop(captured);
    drop(actor);
    drop(hito);

    let hito = open(config(directory, server)).await.unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let replay = hito
        .podcasts
        .unsubscribe(
            &actor,
            &retained.subscription_id.to_string(),
            UnsubscribePodcast {
                request_id: unsubscribe_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.request_id, removed.request_id);
    assert_eq!(replay.subscription_id, removed.subscription_id);
    let replay = hito
        .podcasts
        .subscribe(&actor, original_request())
        .await
        .unwrap();
    assert_eq!(replay.request_id, subscribed.request_id);
    assert_eq!(replay.subscription_id, subscribed.subscription_id);
    assert_eq!(replay.job_id, subscribed.job_id);
    assert_inactive(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_requests(server, 2, 1);

    let resubscribe_id = Uuid::new_v4();
    let reactivated = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: resubscribe_id,
                feed_url: format!("{}/./feed.xml", server.origin),
            },
        )
        .await
        .unwrap();
    assert_eq!(reactivated.request_id, resubscribe_id);
    assert_eq!(reactivated.subscription_id, retained.subscription_id);
    assert!(
        ![subscribed.job_id, refreshed.job_id, queued.job_id].contains(&reactivated.job_id),
        "reactivation must queue genuinely fresh work instead of returning retained history"
    );
    let replay = hito
        .podcasts
        .unsubscribe(
            &actor,
            &retained.subscription_id.to_string(),
            UnsubscribePodcast {
                request_id: unsubscribe_id,
            },
        )
        .await
        .unwrap();
    assert_eq!(replay.request_id, removed.request_id);
    assert_eq!(replay.subscription_id, removed.subscription_id);
    let replay = hito
        .podcasts
        .subscribe(&actor, original_request())
        .await
        .unwrap();
    assert_eq!(replay.request_id, subscribed.request_id);
    assert_eq!(replay.subscription_id, subscribed.subscription_id);
    assert_eq!(replay.job_id, subscribed.job_id);
    assert_active(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    assert_requests(server, 2, 1);
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_active(&hito, &actor, &retained).await;
    assert_private_state(&hito, &actor, &retained).await;
    let playback = hito
        .playback
        .preferred_playback_source(&actor, &retained.recording_id.to_string())
        .await
        .unwrap();
    assert_eq!(playback.source_id(), retained.source_id.to_string());
    assert_requests(server, 3, 1);
    assert_wav(&playback, wav).await;
    assert_private_state(&hito, &actor, &retained).await;
    assert_requests(server, 3, 2);
}

fn config(directory: &Path, server: &PodcastAudioServer) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-subscription-lifecycle").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    config
}

async fn assert_active(hito: &Services, actor: &Actor, retained: &RetainedEpisode) {
    let subscriptions = hito.podcasts.subscriptions(actor).await.unwrap();
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions[0].subscription_id, retained.subscription_id);
    let episodes = hito
        .podcasts
        .episodes(
            actor,
            &retained.subscription_id.to_string(),
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(episodes.total, 1);
    assert_eq!(episodes.items.len(), 1);
    assert_eq!(episodes.items[0].episode_id, retained.episode_id);
    assert_eq!(
        episodes.items[0].recording.id,
        retained.recording_id.to_string()
    );
    let sources = hito
        .catalog
        .sources(actor, &retained.recording_id.to_string())
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, retained.source_id.to_string());
    assert_eq!(sources[0].provider, "podcast");
    assert!(sources[0].ready && sources[0].device_id.is_none());
}

async fn assert_inactive(hito: &Services, actor: &Actor, retained: &RetainedEpisode) {
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
    assert!(matches!(
        hito.playback
            .playback_source(
                actor,
                &retained.recording_id.to_string(),
                &retained.source_id.to_string(),
            )
            .await,
        Err(Error::NotFound)
    ));
    let sources = hito
        .catalog
        .sources(actor, &retained.recording_id.to_string())
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, retained.source_id.to_string());
    assert!(!sources[0].ready);
}

async fn assert_private_state(hito: &Services, actor: &Actor, retained: &RetainedEpisode) {
    let recordings = hito
        .catalog
        .recordings_by_ids(actor, &[retained.recording_id])
        .await
        .unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].title, PRIVATE_TITLE);
    assert_eq!(recordings[0].inherited_title, fixture::TITLE);
    assert_eq!(recordings[0].artist, fixture::AUTHOR);
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
    for (index, entry) in playlist.entries.iter().enumerate() {
        assert_eq!(entry.entry_id, retained.entries[index].to_string());
        assert_eq!(
            entry.recording_id.as_deref(),
            Some(retained.recording_id.to_string().as_str())
        );
        assert_eq!(entry.position, index as i64);
        assert_eq!(entry.title, PRIVATE_TITLE);
        assert!(entry.liked);
    }
    assert!(matches!(
        hito.downloads
            .status(actor, &retained.recording_id.to_string())
            .await,
        Err(Error::NotFound)
    ));
}

async fn assert_wav(grant: &PlaybackSource, expected: &[u8]) {
    let OpenedAudio::Content(mut audio) = grant.open(AudioRequest::default()).await.unwrap() else {
        panic!("explicit lifecycle playback must return full audio content");
    };
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(expected.len() as u64));
    let body = audio
        .body
        .as_mut()
        .expect("explicit playback must expose an audio body");
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= expected.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, expected);
}

fn assert_requests(server: &PodcastAudioServer, feeds: usize, enclosures: usize) {
    assert_eq!(server.feed_requests(), feeds);
    assert_eq!(server.enclosure_requests(), enclosures);
    assert_eq!(server.requests(), feeds + enclosures);
    assert_eq!(server.unexpected(), 0);
}
