#[path = "fixtures/podcast_audio_service.rs"]
mod fixture;
mod support;

use fixture::PodcastAudioServer;
use futures_util::FutureExt;
use hito_core::{
    Action, Actor, AudioRequest, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PodcastEpisodeQuery, PodcastNetworkConfig, Services, SubscribePodcast, open,
};
use std::{panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn a_refreshed_episode_plays_through_its_authorized_catalog_source() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let original = support::write_wav(&directory.path().join("owned-fixture.wav"), 211);
    let mut server = PodcastAudioServer::start(original.clone()).await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(exercise(directory.path(), &server, &original)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    assert!(
        joined,
        "the owned RSS and audio service must shut down and join on success or panic"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public podcast playback exercise must finish within its deadline"),
    }
}

async fn exercise(directory: &Path, server: &PodcastAudioServer, original: &[u8]) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-playback").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "podcast-owner".into(),
            password: "owned podcast playback password".into(),
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
        server.requests() == 0,
        "subscribing must queue metadata work without any network request"
    );
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap(),
        "the owned feed refresh must execute real queued work"
    );
    let subscription = receipt.subscription_id.to_string();
    let episodes = hito
        .podcasts
        .episodes(&actor, &subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert!(
        episodes.total == 1 && episodes.items.len() == 1,
        "the successful metadata refresh must publish exactly one episode"
    );
    let episode = &episodes.items[0];
    assert!(
        episode.recording.inherited_title == fixture::TITLE
            && episode.recording.artist == fixture::AUTHOR,
        "the owned RSS metadata must be published before playback is attempted"
    );
    let recording_id = Uuid::parse_str(&episode.recording.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "My private playable episode".into(),
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
    assert_personal_state(
        &hito,
        &actor,
        &subscription,
        episode.episode_id,
        recording_id,
    )
    .await;
    let selected = hito
        .catalog
        .preferred_playback_source(&actor, &episode.recording.id)
        .await;
    assert!(
        server.requests() == 1
            && server.feed_requests() == 1
            && server.enclosure_requests() == 0
            && server.unexpected() == 0,
        "metadata reads and playback selection must not open the enclosure"
    );
    assert!(
        matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ),
        "metadata refresh and source selection must not create a persistent download"
    );
    let grant = selected.unwrap_or_else(|_| {
        panic!("a refreshed episode must have an authorized preferred playback source")
    });
    assert!(
        !grant.is_local(),
        "the RSS source must use the remote audio path"
    );
    let source_id =
        Uuid::parse_str(grant.source_id()).expect("the playback source must have a Hito UUID");
    assert!(
        !source_id.is_nil(),
        "the podcast playback source must have its own stable identity"
    );
    let sources = hito
        .catalog
        .sources(&actor, &episode.recording.id)
        .await
        .unwrap();
    assert!(
        sources.len() == 1
            && sources[0].id == grant.source_id()
            && sources[0].recording_id == episode.recording.id
            && sources[0].provider == "podcast"
            && sources[0].label == "Podcast audio"
            && sources[0].device_id.is_none()
            && sources[0].ready
            && !sources[0].can_prepare,
        "the public source must describe the authorized podcast enclosure without a plugin binding"
    );
    let public_sources = serde_json::to_string(&sources).unwrap();
    assert!(
        !public_sources.contains(&server.enclosure_url)
            && !public_sources.contains("podcast-private-enclosure-canary"),
        "public source facts must never expose an enclosure address or its private query"
    );
    assert!(
        server.requests() == 1,
        "reading source facts must not perform network discovery or audio IO"
    );

    let OpenedAudio::Content(mut audio) = grant
        .open(AudioRequest::default())
        .await
        .unwrap_or_else(|_| panic!("explicit playback must open the owned enclosure"))
    else {
        panic!("explicit full playback must return content rather than an unsatisfiable range");
    };
    assert!(
        audio.content_type == "audio/wav"
            && audio.length == Some(original.len() as u64)
            && audio.total_length == Some(original.len() as u64)
            && audio.range.is_none()
            && audio.seekable,
        "the relay must expose the actual owned WAV representation and truthful framing"
    );
    let body = audio
        .body
        .as_mut()
        .expect("explicit playback must return a readable audio body");
    let mut received = Vec::new();
    while let Some(chunk) = body
        .chunk()
        .await
        .unwrap_or_else(|_| panic!("the owned audio body must remain readable"))
    {
        assert!(
            chunk.len() <= original.len().saturating_sub(received.len()),
            "the owned playback body must stay within the expected representation length"
        );
        received.extend_from_slice(&chunk);
    }
    assert!(
        received == original,
        "explicit playback must return the exact deterministic PCM WAV bytes"
    );
    assert!(
        body.chunk().await.unwrap().is_none(),
        "a completed playback body must remain complete"
    );
    assert!(
        server.requests() == 2
            && server.feed_requests() == 1
            && server.enclosure_requests() == 1
            && server.unexpected() == 0,
        "only the explicit audio open may add the single enclosure request"
    );
    assert!(
        matches!(
            hito.downloads.status(&actor, &episode.recording.id).await,
            Err(Error::NotFound)
        ),
        "playing the enclosure must not create a persistent download"
    );
    assert_personal_state(
        &hito,
        &actor,
        &subscription,
        episode.episode_id,
        recording_id,
    )
    .await;
}

async fn assert_personal_state(
    hito: &Services,
    actor: &Actor,
    subscription: &str,
    episode_id: Uuid,
    recording_id: Uuid,
) {
    let episodes = hito
        .podcasts
        .episodes(actor, subscription, PodcastEpisodeQuery::default())
        .await
        .unwrap();
    assert!(
        episodes.total == 1
            && episodes.items.len() == 1
            && episodes.items[0].episode_id == episode_id
            && episodes.items[0].recording.id == recording_id.to_string(),
        "playback must preserve the original subscription, episode and recording identities"
    );
    let catalog = hito
        .catalog
        .recordings_by_ids(actor, &[recording_id])
        .await
        .unwrap();
    assert!(
        catalog.len() == 1
            && catalog[0].title == "My private playable episode"
            && catalog[0].inherited_title == fixture::TITLE
            && catalog[0].artist == fixture::AUTHOR
            && catalog[0].liked,
        "playback must preserve personal metadata, its inherited provenance and the saved like"
    );
    let progress = hito
        .collection
        .saved_position(actor, recording_id)
        .await
        .unwrap()
        .expect("the account's explicit progress must remain available");
    assert!(
        progress.position_ms == 700,
        "opening audio must not overwrite saved account progress"
    );
    let activity = hito.collection.activity(actor).await.unwrap();
    assert!(
        activity.history.len() == 1
            && activity.history[0].id == recording_id.to_string()
            && activity.history[0].speed == 1.5,
        "opening audio must preserve the account's existing listening history and speed"
    );
}
