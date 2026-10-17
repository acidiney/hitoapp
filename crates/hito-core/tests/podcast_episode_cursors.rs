#[path = "fixtures/podcast_service.rs"]
#[allow(dead_code)]
mod fixture;
mod support;

use hito_core::{
    Action, CoreConfig, Credentials, Error, HostDeviceId, PodcastEpisodeQuery,
    PodcastNetworkConfig, RefreshPodcast, SubscribePodcast, open,
};
use uuid::Uuid;

#[tokio::test]
async fn returned_episode_cursors_expire_after_refresh_without_losing_private_state() {
    let server = fixture::PodcastServer::start().await;
    let other_feed = fixture::PodcastServer::start().await;
    server.publish_two_episodes("Owned cursor author").await;
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("podcast-episode-cursors").unwrap(),
    );
    config.podcast_network =
        PodcastNetworkConfig::new(vec![server.origin.clone(), other_feed.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "cursor-owner".into(),
            password: "owned podcast cursor password".into(),
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
    let mut first_page = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(first_page.total, 2);
    assert_eq!(first_page.items.len(), 1);
    let first = first_page.items.remove(0);
    let old_cursor = first_page.next_cursor.unwrap();
    let mut second_page = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: Some(old_cursor.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(second_page.total, 2);
    assert_eq!(second_page.items.len(), 1);
    assert!(second_page.next_cursor.is_none());
    let second = second_page.items.remove(0);
    assert_ne!(first.episode_id, second.episode_id);
    let recording_id = Uuid::parse_str(&first.recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private cursor episode".into(),
            },
            Action::RecordingArtistSet {
                recording_id,
                artist: "My private cursor author".into(),
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

    server.publish_two_episodes("Revised cursor author").await;
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
    assert!(matches!(
        hito.podcasts
            .episodes(
                &actor,
                &subscription,
                PodcastEpisodeQuery {
                    limit: Some(1),
                    after: Some(old_cursor.clone()),
                }
            )
            .await,
        Err(Error::Conflict(_))
    ));

    let mut current = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(1),
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(current.total, 2);
    assert_eq!(current.items.len(), 1);
    let retained = current.items.remove(0);
    assert_eq!(retained.episode_id, first.episode_id);
    assert_eq!(retained.recording.id, first.recording.id);
    assert_eq!(retained.recording.title, "My private cursor episode");
    assert_eq!(retained.recording.artist, "My private cursor author");
    assert_eq!(retained.recording.inherited_artist, "Revised cursor author");
    assert!(retained.recording.liked);
    assert_eq!(
        hito.collection
            .saved_position(&actor, recording_id)
            .await
            .unwrap()
            .unwrap()
            .position_ms,
        700
    );
    let current_cursor = current.next_cursor.unwrap();
    assert_ne!(current_cursor, old_cursor);
    let continued = hito
        .podcasts
        .episodes(
            &actor,
            &subscription,
            PodcastEpisodeQuery {
                limit: Some(2),
                after: Some(current_cursor.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(continued.total, 2);
    assert_eq!(continued.items.len(), 1);
    assert_eq!(continued.items[0].episode_id, second.episode_id);
    assert_eq!(continued.items[0].recording.id, second.recording.id);
    assert!(continued.next_cursor.is_none());

    assert!(matches!(
        hito.podcasts
            .episodes(
                &actor,
                &subscription,
                PodcastEpisodeQuery {
                    limit: Some(1),
                    after: Some("not-a-Hito-cursor!".into()),
                }
            )
            .await,
        Err(Error::Invalid(_))
    ));
    let credentials = || Credentials {
        username: "cursor-other".into(),
        password: "owned other cursor password".into(),
    };
    hito.identity
        .create_user(&actor, credentials())
        .await
        .unwrap();
    let other_session = hito.identity.login(credentials()).await.unwrap();
    let other_actor = hito
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    assert!(matches!(
        hito.podcasts
            .episodes(
                &other_actor,
                &subscription,
                PodcastEpisodeQuery {
                    limit: Some(1),
                    after: Some(current_cursor.clone()),
                }
            )
            .await,
        Err(Error::NotFound)
    ));
    let other = hito
        .podcasts
        .subscribe(
            &actor,
            SubscribePodcast {
                request_id: Uuid::new_v4(),
                feed_url: other_feed.feed_url.clone(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        hito.podcasts
            .episodes(
                &actor,
                &other.subscription_id.to_string(),
                PodcastEpisodeQuery {
                    limit: Some(1),
                    after: Some(current_cursor),
                }
            )
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(server.feed_requests(), 2);
    assert_eq!(other_feed.feed_requests(), 0);
    assert_eq!(server.enclosure_requests(), 0);
    assert_eq!(other_feed.enclosure_requests(), 0);
}
