use hito_core::{
    AudioRequest, BeginPlaybackLease, Error, OpenedAudio,
    test_support::{hang_guard::hang_guard, provider_audio::AudioLibrary},
};
use std::time::Duration;

#[tokio::test]
async fn lease_warmed_response_is_reused_by_open_audio_after_lease_release() {
    let prefix_size = 128;
    let (f, held_tail, requests_before_lease) =
        hang_guard("owned provider direct adoption setup", async {
            let f = AudioLibrary::start().await;
            f.ready().await;
            let held_tail = f.server.hold_audio_tail(prefix_size);
            let requests_before_lease = f.server.requests("v1/audio").await.len();
            (f, held_tail, requests_before_lease)
        })
        .await;

    let mut lease_id = None;
    let mut upstream_started_before_playback = false;
    let mut prefix_was_available_while_tail_held = false;
    let mut response_shape_is_expected = false;
    let mut lease_release_completed_while_tail_held = false;
    let mut lease_is_gone = false;
    let mut playback_completed = false;
    let mut playback_bytes = Vec::new();

    if let Ok(Ok(lease)) = tokio::time::timeout(
        Duration::from_secs(5),
        f.hito.playback.begin_lease(
            &f.actor,
            BeginPlaybackLease {
                recording_id: f.source.recording_id.clone(),
                source_id: f.source.id.clone(),
            },
        ),
    )
    .await
    {
        lease_id = Some(lease.id);
        if tokio::time::timeout(Duration::from_secs(2), held_tail.entered())
            .await
            .is_ok()
        {
            upstream_started_before_playback =
                tokio::time::timeout(Duration::from_secs(2), f.server.requests("v1/audio"))
                    .await
                    .ok()
                    .is_some_and(|requests| {
                        requests.len() == requests_before_lease + 1
                            && requests.get(requests_before_lease).is_some_and(
                                |(headers, query)| {
                                    query.get("_method").and_then(serde_json::Value::as_str)
                                        == Some("GET")
                                        && headers.get("range").is_none()
                                },
                            )
                    });
        }

        if let Ok(Ok(OpenedAudio::Content(mut audio))) = tokio::time::timeout(
            Duration::from_secs(5),
            f.hito
                .playback
                .open_audio(&f.actor, &f.source.id, AudioRequest::default()),
        )
        .await
        {
            response_shape_is_expected = audio.content_type == "audio/wav"
                && audio.length == Some(f.original.len() as u64)
                && audio.total_length == Some(f.original.len() as u64);
            if let Some(mut body) = audio.body.take() {
                let prefix = tokio::time::timeout(Duration::from_secs(5), async {
                    let mut prefix = Vec::new();
                    while prefix.len() < prefix_size {
                        let Some(chunk) = body.chunk().await? else {
                            return Err(Error::NotFound);
                        };
                        prefix.extend_from_slice(&chunk);
                    }
                    Ok::<_, Error>(prefix)
                })
                .await;
                if let Ok(Ok(prefix)) = prefix {
                    prefix_was_available_while_tail_held = prefix
                        .get(..prefix_size)
                        .is_some_and(|prefix| prefix == &f.original[..prefix_size])
                        && !held_tail.released();
                    playback_bytes.extend_from_slice(&prefix);
                }

                if let Some(id) = lease_id {
                    lease_release_completed_while_tail_held = matches!(
                        tokio::time::timeout(
                            Duration::from_secs(5),
                            f.hito.playback.release_lease(&f.actor, id),
                        )
                        .await,
                        Ok(Ok(()))
                    ) && !held_tail.released();
                    lease_is_gone = matches!(
                        tokio::time::timeout(
                            Duration::from_secs(2),
                            f.hito.playback.lease_status(&f.actor, id),
                        )
                        .await,
                        Ok(Err(Error::NotFound))
                    );
                }

                held_tail.release();
                if prefix_was_available_while_tail_held {
                    if let Ok(Ok(remaining)) = tokio::time::timeout(Duration::from_secs(5), async {
                        let mut remaining = Vec::new();
                        while let Some(chunk) = body.chunk().await? {
                            remaining.extend_from_slice(&chunk);
                        }
                        Ok::<_, Error>(remaining)
                    })
                    .await
                    {
                        playback_bytes.extend_from_slice(&remaining);
                        playback_completed = true;
                    }
                }
                drop(body);
            }
        }
    }

    held_tail.release();
    let requests = tokio::time::timeout(Duration::from_secs(2), f.server.requests("v1/audio"))
        .await
        .ok();
    let only_one_get_without_range = requests.as_ref().is_some_and(|requests| {
        requests.len() == requests_before_lease + 1
            && requests
                .get(requests_before_lease)
                .is_some_and(|(headers, query)| {
                    query.get("_method").and_then(serde_json::Value::as_str) == Some("GET")
                        && headers.get("range").is_none()
                })
    });
    let download_status = tokio::time::timeout(
        Duration::from_secs(2),
        f.hito.downloads.status(&f.actor, &f.source.recording_id),
    )
    .await
    .ok();
    let source_bytes = std::fs::read(f.music.join("original.wav"));

    let playback_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.playback.close()).await,
        Ok(Ok(()))
    );
    let replicas_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.replicas.close()).await,
        Ok(Ok(()))
    );
    let replication_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.catalog_replication.close()).await,
        Ok(Ok(()))
    );
    let server_closed = tokio::time::timeout(Duration::from_secs(5), f.server.shutdown())
        .await
        .is_ok();

    assert!(
        playback_closed && replicas_closed && replication_closed && server_closed,
        "owned provider audio services must close within their teardown deadlines"
    );
    assert!(upstream_started_before_playback);
    assert!(prefix_was_available_while_tail_held);
    assert!(response_shape_is_expected);
    assert!(lease_release_completed_while_tail_held);
    assert!(lease_is_gone);
    assert!(playback_completed);
    assert_eq!(playback_bytes, f.original);
    assert!(only_one_get_without_range);
    assert!(matches!(download_status, Some(Err(Error::NotFound))));
    assert_eq!(source_bytes.unwrap(), f.original);
}
