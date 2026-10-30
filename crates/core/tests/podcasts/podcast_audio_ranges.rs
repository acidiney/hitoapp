#[path = "../fixtures/podcast_range_service.rs"]
mod fixture;
use hito_core::test_support::support;

use fixture::PodcastRangeServer;
use futures_util::FutureExt;
use hito_core::{
    Actor, AudioRequest, ByteRange, CoreConfig, Credentials, Error, HostDeviceId, OpenedAudio,
    PodcastEpisodeQuery, PodcastNetworkConfig, Services, SubscribePodcast, open,
};
use std::{ops::RangeInclusive, panic::AssertUnwindSafe, path::Path, time::Duration};
use uuid::Uuid;

#[tokio::test]
async fn public_podcast_audio_preserves_head_range_suffix_and_unsatisfiable_representations() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("music")).unwrap();
    let original = support::write_wav(&directory.path().join("owned-range-fixture.wav"), 193);
    let mut server = PodcastRangeServer::start(original.clone()).await;
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(exercise(directory.path(), &server, &original)).catch_unwind(),
    )
    .await;
    assert!(
        server.finish().await,
        "the owned range service must shut down and join on success or panic"
    );
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the public podcast representation exercise must finish within its deadline")
        }
    }
}

async fn exercise(directory: &Path, server: &PodcastRangeServer, original: &[u8]) {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("podcast-audio-ranges").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![server.origin.clone()]).unwrap();
    let hito = open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "range-owner".into(),
            password: "owned range fixture password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let subscription = hito
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
        server.seen().is_empty(),
        "subscribing must only queue metadata work"
    );
    assert!(
        hito.podcasts
            .run_next_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let episodes = hito
        .podcasts
        .episodes(
            &actor,
            &subscription.subscription_id.to_string(),
            PodcastEpisodeQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(episodes.total, 1);
    let recording = &episodes.items[0].recording.id;
    let sources = hito.catalog.sources(&actor, recording).await.unwrap();
    assert!(sources.len() == 1 && sources[0].provider == "podcast" && sources[0].ready);
    let source = &sources[0].id;
    assert_eq!(
        server.seen().len(),
        1,
        "metadata and source reads must not fetch enclosure audio"
    );
    assert!(matches!(
        hito.downloads.status(&actor, recording).await,
        Err(Error::NotFound)
    ));

    let selected = ByteRange::From {
        first: fixture::FIRST as u64,
        last: Some(fixture::LAST as u64),
    };
    let OpenedAudio::Content(head) = hito
        .playback
        .open_audio(
            &actor,
            source,
            AudioRequest {
                head: true,
                range: Some(selected),
            },
        )
        .await
        .unwrap()
    else {
        panic!("HEAD must describe the complete audio representation");
    };
    assert!(head.body.is_none() && head.range.is_none() && head.seekable);
    assert_eq!(head.content_type, "audio/wav");
    assert_eq!(head.length, Some(original.len() as u64));
    assert_eq!(head.total_length, Some(original.len() as u64));
    assert_selected(
        &hito,
        &actor,
        source,
        selected,
        fixture::FIRST..=fixture::LAST,
        original,
    )
    .await;
    assert_selected(
        &hito,
        &actor,
        source,
        ByteRange::Suffix {
            length: fixture::SUFFIX as u64,
        },
        original.len() - fixture::SUFFIX..=original.len() - 1,
        original,
    )
    .await;
    let unsatisfiable = hito
        .playback
        .open_audio(
            &actor,
            source,
            AudioRequest {
                head: false,
                range: Some(ByteRange::From {
                    first: original.len() as u64 + 5,
                    last: None,
                }),
            },
        )
        .await
        .unwrap();
    assert!(
        matches!(unsatisfiable, OpenedAudio::Unsatisfiable { length: Some(length) }
        if length == original.len() as u64),
        "416 must report the actual total representation length"
    );
    assert!(
        matches!(
            hito.downloads.status(&actor, recording).await,
            Err(Error::NotFound)
        ),
        "HEAD and range playback must never create a persistent download"
    );

    let seen = server.seen();
    assert_eq!(
        seen.len(),
        5,
        "only one feed read and four explicit audio operations may reach the service"
    );
    let expected = [
        (
            "GET",
            "/feed.xml",
            "application/rss+xml, application/atom+xml, application/xml, text/xml;q=0.9",
            None,
        ),
        (
            "HEAD",
            fixture::ENCLOSURE_TARGET,
            "audio/*,application/ogg",
            None,
        ),
        (
            "GET",
            fixture::ENCLOSURE_TARGET,
            "audio/*,application/ogg",
            Some(format!("bytes={}-{}", fixture::FIRST, fixture::LAST)),
        ),
        (
            "GET",
            fixture::ENCLOSURE_TARGET,
            "audio/*,application/ogg",
            Some(format!("bytes=-{}", fixture::SUFFIX)),
        ),
        (
            "GET",
            fixture::ENCLOSURE_TARGET,
            "audio/*,application/ogg",
            Some(format!("bytes={}-", original.len() + 5)),
        ),
    ];
    for (actual, (method, target, accept, range)) in seen.iter().zip(expected) {
        assert!(actual.method.as_str() == method && actual.target == target);
        assert_eq!(actual.headers.get("range"), range.as_ref());
        for (name, value) in [
            ("host", server.origin.trim_start_matches("http://")),
            ("accept", accept),
            ("accept-encoding", "identity"),
            ("connection", "close"),
            ("user-agent", concat!("Hito/", env!("CARGO_PKG_VERSION"))),
        ] {
            assert!(
                actual
                    .headers
                    .get(name)
                    .is_some_and(|header| header == value),
                "the real TCP request must contain the exact safe transport headers"
            );
        }
        assert!(
            !actual.sensitive_headers,
            "account credentials, cookies and plugin protocol headers must never reach an RSS enclosure"
        );
    }
}

async fn assert_selected(
    hito: &Services,
    actor: &Actor,
    source: &str,
    selection: ByteRange,
    expected: RangeInclusive<usize>,
    original: &[u8],
) {
    let OpenedAudio::Content(mut audio) = hito
        .playback
        .open_audio(
            actor,
            source,
            AudioRequest {
                head: false,
                range: Some(selection),
            },
        )
        .await
        .unwrap()
    else {
        panic!("a satisfiable explicit range must return content");
    };
    let selected = &original[expected.clone()];
    assert_eq!(audio.content_type, "audio/wav");
    assert_eq!(audio.length, Some(selected.len() as u64));
    assert_eq!(audio.total_length, Some(original.len() as u64));
    assert_eq!(
        audio.range,
        Some(*expected.start() as u64..=*expected.end() as u64)
    );
    assert!(audio.seekable);
    let body = audio
        .body
        .as_mut()
        .expect("range content must provide an audio body");
    let mut received = Vec::new();
    while let Some(chunk) = body.chunk().await.unwrap() {
        assert!(chunk.len() <= selected.len().saturating_sub(received.len()));
        received.extend_from_slice(&chunk);
    }
    assert!(
        received == selected,
        "the relay must return exactly the requested owned WAV bytes"
    );
    assert!(body.chunk().await.unwrap().is_none());
}
