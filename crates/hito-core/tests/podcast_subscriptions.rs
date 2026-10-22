#[path = "fixtures/podcast_service.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Browse, CoreConfig, Credentials, Error, HostDeviceId, PodcastEpisodeQuery,
    PodcastNetworkConfig, RefreshPodcast, SubscribePodcast, open,
};
use std::path::Path;
use uuid::Uuid;

fn config(directory: &Path, origin: &str) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-subscriptions").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![origin.to_owned()]).unwrap();
    config
}

#[tokio::test]
async fn oversized_episode_numbers_preserve_the_last_good_snapshot_and_release_the_account_worker()
{
    for overflow in [
        fixture::NumberOverflow::EnclosureLength,
        fixture::NumberOverflow::Duration,
    ] {
        let server = fixture::PodcastServer::start().await;
        let other = fixture::PodcastServer::start().await;
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        let mut core = config(directory.path(), &server.origin);
        core.podcast_network =
            PodcastNetworkConfig::new(vec![server.origin.clone(), other.origin.clone()]).unwrap();
        let hito = open(core).await.unwrap();
        let session = hito
            .identity
            .setup(Credentials {
                username: "podcast-owner".into(),
                password: "owned podcast fixture password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let receipt = hito
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
        assert!(
            hito.podcasts
                .run_next_refresh_until(std::future::pending())
                .await
                .unwrap()
        );
        let subscription = receipt.subscription_id.to_string();
        let episode = hito
            .podcasts
            .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
            .await
            .unwrap()
            .items
            .remove(0);
        let recording_id = Uuid::parse_str(&episode.recording.id).unwrap();
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::RecordingTitleSet {
                    recording_id,
                    title: "My retained episode".into(),
                },
                Action::LikeSet {
                    recording_id,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id,
                    position_ms: 700,
                    speed: 1.5,
                },
            ],
        )
        .await;
        let episodes_before = serde_json::to_value(
            hito.podcasts
                .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
                .await
                .unwrap(),
        )
        .unwrap();
        let catalog_before = serde_json::to_value(
            hito.catalog
                .recordings_by_ids(&actor, &[recording_id])
                .await
                .unwrap(),
        )
        .unwrap();
        let history_before =
            serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap();

        server.publish_numeric_overflow(overflow).await;
        hito.podcasts
            .enqueue_refresh(
                &actor,
                &subscription,
                RefreshPodcast {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        assert!(
            hito.podcasts
                .run_next_refresh_until(std::future::pending())
                .await
                .expect("an invalid feed must finish ordinary failure cleanup")
        );
        assert_eq!(
            serde_json::to_value(
                hito.podcasts
                    .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
                    .await
                    .unwrap()
            )
            .unwrap(),
            episodes_before,
            "{overflow:?}"
        );
        assert_eq!(
            serde_json::to_value(
                hito.catalog
                    .recordings_by_ids(&actor, &[recording_id])
                    .await
                    .unwrap()
            )
            .unwrap(),
            catalog_before,
            "{overflow:?}"
        );
        assert_eq!(
            serde_json::to_value(hito.collection.activity(&actor).await.unwrap()).unwrap(),
            history_before,
            "{overflow:?}"
        );
        assert_eq!(
            hito.collection
                .saved_position(&actor, recording_id)
                .await
                .unwrap()
                .unwrap()
                .position_ms,
            700
        );

        let other_receipt = hito
            .podcasts
            .subscribe(
                &actor,
                SubscribePodcast {
                    request_id: Uuid::new_v4(),
                    feed_url: other.feed_url.clone(),
                },
            )
            .await
            .unwrap();
        assert!(
            hito.podcasts
                .run_next_refresh_until(std::future::pending())
                .await
                .unwrap(),
            "{overflow:?} must not leave an account worker running"
        );
        let other_episodes = hito
            .podcasts
            .episodes(
                &actor,
                &other_receipt.subscription_id.to_string(),
                PodcastEpisodeQuery::default(),
            )
            .await
            .unwrap();
        assert_eq!(other_episodes.total, 1, "{overflow:?}");
        assert_ne!(other_episodes.items[0].recording.id, episode.recording.id);
        assert_eq!(other.feed_requests(), 1);
        assert_eq!(server.enclosure_requests(), 0);
        assert_eq!(other.enclosure_requests(), 0);
    }
}

#[tokio::test]
async fn episode_author_credits_refresh_without_replacing_private_artist_overrides() {
    let server = fixture::PodcastServer::start().await;
    server.publish_two_episodes("Owned podcast author").await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let hito = open(config(directory.path(), &server.origin))
        .await
        .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned podcast fixture password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let receipt = hito
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
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let subscription = receipt.subscription_id.to_string();
    let episodes = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(episodes.total, 2);
    let overridden = &episodes
        .items
        .iter()
        .find(|episode| episode.recording.title == "First owned episode")
        .unwrap()
        .recording;
    let inherited = &episodes
        .items
        .iter()
        .find(|episode| episode.recording.title == "Second owned episode")
        .unwrap()
        .recording;
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(
        artists.total, 1,
        "the subscribed author must appear in the account catalog"
    );
    assert_eq!(artists.items[0].name, "Owned podcast author");
    assert_eq!(artists.items[0].recording_count, 2);
    let original_artist = artists.items[0].id.clone();
    let detail = hito
        .catalog
        .artist(&actor, &original_artist, Browse::default())
        .await
        .unwrap();
    assert_eq!(detail.recordings.total, 2);
    assert!(
        detail
            .recordings
            .items
            .iter()
            .any(|recording| recording.id == overridden.id)
    );
    assert!(
        detail
            .recordings
            .items
            .iter()
            .any(|recording| recording.id == inherited.id)
    );
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&overridden.id).unwrap(),
            artist: "My private podcast author".into(),
        }],
    )
    .await;

    server.publish_two_episodes("Revised podcast author").await;
    hito.podcasts
        .enqueue_refresh(
            &actor,
            &subscription,
            RefreshPodcast {
                request_id: Uuid::new_v4(),
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
    let after = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(after.total, 2);
    let private_recording = &after
        .items
        .iter()
        .find(|episode| episode.recording.id == overridden.id)
        .unwrap()
        .recording;
    assert_eq!(private_recording.artist, "My private podcast author");
    assert_eq!(private_recording.inherited_artist, "Revised podcast author");
    let inherited_recording = &after
        .items
        .iter()
        .find(|episode| episode.recording.id == inherited.id)
        .unwrap()
        .recording;
    assert_eq!(inherited_recording.artist, "Revised podcast author");
    assert_eq!(
        inherited_recording.inherited_artist,
        "Revised podcast author"
    );
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 2);
    assert!(
        artists
            .items
            .iter()
            .all(|artist| artist.name != "Owned podcast author")
    );
    for (name, recording_id) in [
        ("My private podcast author", overridden.id.as_str()),
        ("Revised podcast author", inherited.id.as_str()),
    ] {
        let artist = artists
            .items
            .iter()
            .find(|artist| artist.name == name)
            .unwrap();
        assert_eq!(artist.recording_count, 1);
        let detail = hito
            .catalog
            .artist(&actor, &artist.id, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.recordings.total, 1);
        assert_eq!(detail.recordings.items[0].id, recording_id);
    }
    assert!(matches!(
        hito.catalog
            .artist(&actor, &original_artist, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(server.feed_requests(), 2);
    assert_eq!(server.enclosure_requests(), 0);
}

#[tokio::test]
async fn refreshing_an_episode_preserves_its_identity_and_private_collection_state() {
    let server = fixture::PodcastServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let hito = open(config(directory.path(), &server.origin))
        .await
        .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned podcast fixture password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let receipt = hito
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
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let subscription = receipt.subscription_id.to_string();
    let before = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording_id = Uuid::parse_str(&before.recording.id).unwrap();
    let playlist_id = Uuid::new_v4();
    let occurrences = [Uuid::new_v4(), Uuid::new_v4()];
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private episode title".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 700,
                speed: 1.5,
            },
            Action::PlaylistCreate {
                playlist_id,
                name: "My podcast selection".into(),
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: occurrences[0],
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id,
                entry_id: occurrences[1],
                recording_id,
                position: 1,
            },
        ],
    )
    .await;

    server.revise_episode().await;
    let request_id = Uuid::new_v4();
    let refresh = hito
        .podcasts
        .enqueue_refresh(&actor, &subscription, RefreshPodcast { request_id })
        .await
        .unwrap();
    assert_eq!(refresh.request_id, request_id);
    assert_eq!(refresh.subscription_id, receipt.subscription_id);
    assert_ne!(refresh.job_id, receipt.job_id);
    let replay = hito
        .podcasts
        .enqueue_refresh(&actor, &subscription, RefreshPodcast { request_id })
        .await
        .unwrap();
    assert_eq!(replay.job_id, refresh.job_id);
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );

    let after = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(after.total, 1);
    assert_eq!(after.items[0].episode_id, before.episode_id);
    assert_eq!(after.items[0].recording.id, before.recording.id);
    assert_eq!(
        after.items[0].recording.inherited_title,
        "Revised owned episode"
    );
    assert_eq!(after.items[0].recording.title, "My private episode title");
    assert!(after.items[0].recording.liked);
    let catalog = hito
        .catalog
        .recordings_by_ids(&actor, &[recording_id])
        .await
        .unwrap();
    assert_eq!(catalog[0].id, before.recording.id);
    assert_eq!(catalog[0].inherited_title, "Revised owned episode");
    assert_eq!(catalog[0].title, "My private episode title");
    assert!(catalog[0].liked);
    assert_eq!(
        hito.collection
            .saved_position(&actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        700
    );
    let history = hito.collection.activity(&actor).await.unwrap().history;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, before.recording.id);
    assert_eq!(history[0].speed, 1.5);
    let playlist = hito
        .collection
        .playlist(&actor, &playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), 2);
    for (entry, occurrence) in playlist.entries.iter().zip(occurrences) {
        assert_eq!(entry.entry_id, occurrence.to_string());
        assert_eq!(
            entry.recording_id.as_deref(),
            Some(before.recording.id.as_str())
        );
        assert_eq!(entry.title, "My private episode title");
    }
    assert!(matches!(
        hito.downloads.status(&actor, &before.recording.id).await,
        Err(Error::NotFound)
    ));
    let replay = hito
        .podcasts
        .enqueue_refresh(&actor, &subscription, RefreshPodcast { request_id })
        .await
        .unwrap();
    assert_eq!(replay.request_id, request_id);
    assert_eq!(replay.subscription_id, receipt.subscription_id);
    assert_eq!(replay.job_id, refresh.job_id);
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(server.feed_requests(), 2);
    assert_eq!(server.enclosure_requests(), 0);
}

#[tokio::test]
async fn a_subscription_survives_restart_and_fetches_episodes_without_downloading_audio() {
    let server = fixture::PodcastServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let hito = open(config(directory.path(), &server.origin))
        .await
        .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned podcast fixture password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let request_id = Uuid::new_v4();
    let subscribe = || SubscribePodcast {
        request_id,
        feed_url: server.feed_url.clone(),
    };
    let receipt = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    assert_eq!(receipt.request_id, request_id);
    assert_eq!(server.feed_requests(), 0);
    let replay = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    assert_eq!(replay.subscription_id, receipt.subscription_id);
    assert_eq!(replay.job_id, receipt.job_id);
    assert_eq!(hito.podcasts.subscriptions(&actor).await.unwrap().len(), 1);
    drop(hito);

    let hito = open(config(directory.path(), &server.origin))
        .await
        .unwrap();
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let subscription = receipt.subscription_id.to_string();
    let episodes = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(episodes.total, 1);
    assert_eq!(episodes.items.len(), 1);
    let episode = &episodes.items[0].recording;
    let recording_id = Uuid::parse_str(&episode.id).unwrap();
    assert_eq!(episode.title, "First owned episode");
    let catalog = hito
        .catalog
        .recordings_by_ids(&actor, &[recording_id])
        .await
        .unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].id, episode.id);
    assert_eq!(catalog[0].title, episode.title);
    assert!(matches!(
        hito.downloads.status(&actor, &episode.id).await,
        Err(Error::NotFound)
    ));
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 0);

    let replay = hito.podcasts.subscribe(&actor, subscribe()).await.unwrap();
    assert_eq!(replay.subscription_id, receipt.subscription_id);
    assert_eq!(replay.job_id, receipt.job_id);
    assert_eq!(hito.podcasts.subscriptions(&actor).await.unwrap().len(), 1);
    let retained = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert_eq!(retained.total, 1);
    assert_eq!(retained.items[0].recording.id, episode.id);
    assert!(
        !hito
            .podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(server.feed_requests(), 1);
    assert_eq!(server.enclosure_requests(), 0);
}
