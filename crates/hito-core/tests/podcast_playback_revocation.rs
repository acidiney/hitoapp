#[path = "fixtures/podcast_audio_service.rs"]
mod fixture;
mod support;

use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{
    Action, AudioContent, AudioRequest, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PodcastEpisodeQuery, PodcastNetworkConfig, RefreshPodcast, SubscribePodcast, open,
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn replacing_an_enclosure_revokes_unread_bodies_and_releases_their_stream_slots() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let original = support::write_wav(&directory.path().join("old-owned.wav"), 211);
    let replacement = support::write_wav(&directory.path().join("new-owned.wav"), 977);
    assert!(
        original != replacement,
        "the owned enclosure generations must have distinct PCM bytes"
    );
    let mut server = PodcastAudioServer::start(original).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(exercise(directory.path(), &server, &replacement)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned replacement feed and media service must join on success or panic"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the unread-body revocation exercise must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &PodcastAudioServer, replacement: &[u8]) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-unread-revocation").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned unread podcast password".into(),
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
    let recording = Uuid::parse_str(&episode.recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "My private retained episode".into(),
            },
            Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 700,
                speed: 1.5,
            },
        ],
    )
    .await;
    let old_a = hito
        .catalog
        .preferred_playback_source(&actor, &episode.recording.id)
        .await
        .unwrap();
    let old_b = hito
        .catalog
        .preferred_playback_source(&actor, &episode.recording.id)
        .await
        .unwrap();
    assert!(
        old_a.source_id() == old_b.source_id() && !old_a.is_local() && !old_b.is_local(),
        "both old grants must authorize the same remote podcast source"
    );
    let mut old_bodies = vec![
        content(old_a.open(AudioRequest::default()).await.unwrap()),
        content(old_b.open(AudioRequest::default()).await.unwrap()),
    ];
    assert!(
        old_bodies.iter().all(|audio| audio.body.is_some()),
        "both old audio bodies must remain owned and unread"
    );
    assert!(
        server.feed_requests() == 1
            && server.enclosure_requests() == 2
            && server.requests() == 3
            && server.unexpected() == 0,
        "opening the two old bodies must perform only their two explicit audio requests"
    );
    assert!(
        matches!(
            old_a.open(AudioRequest::default()).await,
            Err(Error::Conflict(_))
        ),
        "a third simultaneous body must hit account stream admission"
    );
    assert!(
        server.requests() == 3,
        "account admission must fail before another enclosure request"
    );

    server.publish_enclosure(replacement.to_vec()).await;
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
    let fresh = hito
        .catalog
        .preferred_playback_source(&actor, &episode.recording.id)
        .await
        .unwrap();
    assert!(
        fresh.source_id() == old_a.source_id(),
        "the same episode GUID must retain its source UUID after a signed URL changes"
    );
    let sources = hito
        .catalog
        .sources(&actor, &episode.recording.id)
        .await
        .unwrap();
    assert!(
        sources.len() == 1
            && sources[0].id == fresh.source_id()
            && sources[0].provider == "podcast"
            && sources[0].label == "Podcast audio"
            && sources[0].ready
            && !sources[0].can_prepare,
        "replacement metadata must preserve one authorized podcast source"
    );
    let wire = serde_json::to_string(&sources).unwrap();
    assert!(
        !wire.contains(&server.enclosure_url) && !wire.contains("podcast-private-enclosure-canary"),
        "neither enclosure generation may escape in public source facts"
    );
    assert!(
        server.feed_requests() == 2 && server.enclosure_requests() == 2 && server.requests() == 4,
        "refresh and fresh selection must not consume or reopen either audio generation"
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut fresh_bodies = Vec::new();
    while fresh_bodies.len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "revocation monitors must release both old slots while the old bodies remain retained and unread"
        );
        match fresh.open(AudioRequest::default()).await {
            Ok(opened) => fresh_bodies.push(content(opened)),
            Err(Error::Conflict(_)) => tokio::time::sleep(Duration::from_millis(25)).await,
            Err(_) => panic!(
                "fresh enclosure authority must remain valid while revoked stream slots are reclaimed"
            ),
        }
    }
    assert!(
        old_bodies.len() == 2
            && old_bodies.iter().all(|audio| audio.body.is_some())
            && fresh_bodies.len() == 2
            && fresh_bodies.iter().all(|audio| audio.body.is_some()),
        "both fresh opens must coexist with the two still-retained unread old bodies"
    );
    assert!(
        server.requests() == 6
            && server.feed_requests() == 2
            && server.enclosure_requests() == 4
            && server.unexpected() == 0,
        "only the two successful fresh opens may add network requests during admission polling"
    );
    for audio in &mut fresh_bodies {
        assert_bytes(audio, replacement).await;
    }
    let settled_requests = server.requests();
    for old in [&old_a, &old_b] {
        assert!(
            matches!(
                old.open(AudioRequest::default()).await,
                Err(Error::NotFound)
            ),
            "old grants must remain revoked after fresh stream capacity is released"
        );
    }
    assert!(
        server.requests() == settled_requests,
        "stale grants must not fetch either their original URL or the current replacement URL"
    );
    for audio in &mut old_bodies {
        let body = audio.body.as_mut().unwrap();
        assert!(
            matches!(body.chunk().await, Err(Error::NotFound)),
            "a retained revoked body must reject its unread original bytes"
        );
        assert!(
            body.chunk().await.unwrap().is_none(),
            "a failed old body must remain terminal"
        );
    }
    let after = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert!(
        after.total == 1
            && after.items.len() == 1
            && after.items[0].episode_id == episode.episode_id
            && after.items[0].recording.id == episode.recording.id
            && after.items[0].recording.title == "My private retained episode"
            && after.items[0].recording.inherited_title == fixture::TITLE
            && after.items[0].recording.liked,
        "enclosure revocation must preserve episode, recording, metadata and like identities"
    );
    let progress = hito
        .collection
        .saved_position(&actor, recording)
        .await
        .unwrap()
        .unwrap();
    let activity = hito.collection.activity(&actor).await.unwrap();
    assert!(
        progress.position_ms == 700
            && activity.history.len() == 1
            && activity.history[0].id == episode.recording.id
            && activity.history[0].speed == 1.5,
        "enclosure replacement and byte reads must preserve saved progress and listening history"
    );
    assert!(
        matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ),
        "changing and playing an enclosure must never create a persistent download"
    );
    assert!(
        server.requests() == settled_requests && server.unexpected() == 0,
        "revoked reads and final private-state checks must perform no provider IO"
    );
}

fn content(opened: OpenedAudio) -> Box<AudioContent> {
    let OpenedAudio::Content(audio) = opened else {
        panic!("owned full audio must return a complete representation");
    };
    audio
}

async fn assert_bytes(audio: &mut AudioContent, expected: &[u8]) {
    assert!(
        audio.length == Some(expected.len() as u64)
            && audio.total_length == Some(expected.len() as u64)
            && audio.content_type == "audio/wav"
            && audio.range.is_none(),
        "each fresh body must describe the replacement WAV with truthful framing"
    );
    let body = audio
        .body
        .as_mut()
        .expect("each fresh open must hold an audio body");
    let mut received = Vec::new();
    while let Some(chunk) = body
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("replacement PCM must remain readable"))
    {
        assert!(
            chunk.len() <= 65536 && chunk.len() <= expected.len().saturating_sub(received.len()),
            "replacement body chunks must remain bounded by the actual representation"
        );
        received.extend_from_slice(&chunk);
    }
    assert!(
        received == expected,
        "every fresh body must return the exact distinct replacement PCM bytes"
    );
}
