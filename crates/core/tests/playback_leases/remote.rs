use super::fixture::content;
use axum::http::StatusCode;
use hito_core::{
    AudioRequest, BeginPlaybackLease, ByteRange, Error, OpenedAudio,
    test_support::{
        hang_guard::hang_guard,
        provider_audio::{AudioLibrary, headers},
    },
};
use std::time::Duration;

#[tokio::test]
async fn lease_opens_upstream_before_playback_and_reuses_its_response() {
    let prefix_size = 128;
    let (f, held_tail, requests_before_lease) = hang_guard("owned provider audio setup", async {
        let f = AudioLibrary::start().await;
        f.ready().await;
        let held_tail = f.server.hold_audio_tail(prefix_size);
        let requests_before_lease = f.server.requests("v1/audio").await.len();
        (f, held_tail, requests_before_lease)
    })
    .await;

    let mut lease_created = false;
    let mut upstream_started_before_playback = false;
    let mut prefix_was_available_while_tail_held = false;
    let mut playback_completed = false;
    let mut playback_bytes = Vec::new();

    match tokio::time::timeout(
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
        Ok(Ok(lease)) => {
            lease_created = true;
            upstream_started_before_playback =
                tokio::time::timeout(Duration::from_secs(2), held_tail.entered())
                    .await
                    .is_ok();

            match tokio::time::timeout(
                Duration::from_secs(5),
                f.hito
                    .playback
                    .open_lease_audio(&f.actor, lease.id, AudioRequest::default()),
            )
            .await
            {
                Ok(Ok(OpenedAudio::Content(mut audio))) => {
                    if let Some(mut body) = audio.body.take() {
                        if let Ok(Ok(prefix)) =
                            tokio::time::timeout(Duration::from_secs(5), async {
                                let mut prefix = Vec::new();
                                while prefix.len() < prefix_size {
                                    let Some(chunk) = body.chunk().await? else {
                                        return Err(Error::NotFound);
                                    };
                                    prefix.extend_from_slice(&chunk);
                                }
                                Ok::<_, Error>(prefix)
                            })
                            .await
                        {
                            prefix_was_available_while_tail_held = prefix
                                .get(..prefix_size)
                                .is_some_and(|prefix| prefix == &f.original[..prefix_size])
                                && !held_tail.released();
                            playback_bytes.extend_from_slice(&prefix);
                            held_tail.release();
                            match tokio::time::timeout(Duration::from_secs(5), async {
                                let mut remaining = Vec::new();
                                while let Some(chunk) = body.chunk().await? {
                                    remaining.extend_from_slice(&chunk);
                                }
                                Ok::<_, Error>(remaining)
                            })
                            .await
                            {
                                Ok(Ok(remaining)) => {
                                    playback_bytes.extend_from_slice(&remaining);
                                    playback_completed = true;
                                }
                                Ok(Err(_)) | Err(_) => {}
                            }
                        }
                    }
                }
                Ok(Ok(OpenedAudio::Unsatisfiable { .. })) | Ok(Err(_)) | Err(_) => {}
            }
        }
        Ok(Err(_)) | Err(_) => {}
    }

    held_tail.release();
    let request_count = tokio::time::timeout(Duration::from_secs(2), f.server.requests("v1/audio"))
        .await
        .ok()
        .map(|requests| requests.len());
    let download_status = tokio::time::timeout(
        Duration::from_secs(2),
        f.hito.downloads.status(&f.actor, &f.source.recording_id),
    )
    .await
    .ok();
    let source_bytes = std::fs::read(&f.music.join("original.wav"));

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
    assert!(
        lease_created,
        "the authorized provider lease must be created"
    );
    assert!(
        prefix_was_available_while_tail_held,
        "the lease must deliver its retained prefix before the upstream tail is released"
    );
    assert!(
        playback_completed,
        "the retained response tail must drain after release"
    );
    assert_eq!(playback_bytes, f.original);
    assert_eq!(request_count, Some(requests_before_lease + 1));
    assert!(matches!(download_status, Some(Err(Error::NotFound))));
    assert_eq!(source_bytes.unwrap(), f.original);
    assert!(
        upstream_started_before_playback,
        "an authorized lease must start an upstream response before playback opens"
    );
}

#[tokio::test]
async fn provider_leases_reuse_ranged_relay_and_retire_with_actual_credential_authority() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.server.range_chunks(81).await;
    let mut response_headers = headers();
    response_headers.insert(
        "etag",
        "\"actual-provider-representation\"".parse().unwrap(),
    );
    f.server
        .audio(StatusCode::OK, response_headers, f.original.clone())
        .await;
    let lease = f
        .hito
        .playback
        .begin_lease(
            &f.actor,
            BeginPlaybackLease {
                recording_id: f.source.recording_id.clone(),
                source_id: f.source.id.clone(),
            },
        )
        .await
        .unwrap();
    let opened = f
        .hito
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: false,
                range: Some(ByteRange::From {
                    first: 10,
                    last: Some(90),
                }),
            },
        )
        .await
        .unwrap();
    if let OpenedAudio::Content(audio) = &opened {
        assert_ne!(
            audio.validators,
            hito_core::AudioValidators::default(),
            "the actual provider validator must survive the lease"
        );
    }
    assert_eq!(content(opened).await, f.original[10..=90]);
    let OpenedAudio::Content(mut audio) = f
        .hito
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    f.hito
        .plugins
        .clear_credential(&f.actor, &f.installation.id, 1)
        .await
        .unwrap();
    assert!(body.chunk().await.is_err());
    assert!(
        f.hito
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
    f.hito.playback.close().await.unwrap();
    f.hito.replicas.close().await.unwrap();
    f.hito.catalog_replication.close().await.unwrap();
    f.server.shutdown().await;
}

#[tokio::test]
async fn temporary_provider_open_failure_does_not_fabricate_authority_withdrawal() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let lease = f
        .hito
        .playback
        .begin_lease(
            &f.actor,
            BeginPlaybackLease {
                recording_id: f.source.recording_id.clone(),
                source_id: f.source.id.clone(),
            },
        )
        .await
        .unwrap();
    f.server
        .audio_statuses(vec![StatusCode::SERVICE_UNAVAILABLE]);
    assert!(matches!(
        f.hito
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await,
        Err(Error::Upstream(_))
    ));
    assert_eq!(
        f.hito
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .unwrap(),
        lease
    );
    let opened = f
        .hito
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap();
    assert_eq!(content(opened).await, f.original);
    f.hito.playback.close().await.unwrap();
    f.hito.replicas.close().await.unwrap();
    f.hito.catalog_replication.close().await.unwrap();
    f.server.shutdown().await;
}
