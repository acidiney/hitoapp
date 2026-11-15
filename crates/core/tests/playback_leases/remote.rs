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
    let f = hang_guard("owned temporary provider failure setup", async {
        let f = AudioLibrary::start().await;
        f.ready().await;
        f.server
            .audio(
                StatusCode::SERVICE_UNAVAILABLE,
                headers(),
                f.original.clone(),
            )
            .await;
        f
    })
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
        .ok();
    let first_open = if let Some(lease) = &lease {
        Some(
            f.hito
                .playback
                .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
                .await,
        )
    } else {
        None
    };
    let first_open_error_code = first_open
        .as_ref()
        .and_then(|result| result.as_ref().err().map(Error::code))
        .unwrap_or("not_attempted");
    let first_open_was_upstream = matches!(first_open.as_ref(), Some(Err(Error::Upstream(_))));
    drop(first_open);
    let lease_status_preserved = if let Some(lease) = &lease {
        matches!(
            f.hito.playback.lease_status(&f.actor, lease.id).await,
            Ok(status) if status == *lease
        )
    } else {
        false
    };
    f.ready().await;
    let second_open = if let Some(lease) = &lease {
        Some(
            f.hito
                .playback
                .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
                .await,
        )
    } else {
        None
    };
    let second_open_error_code = second_open
        .as_ref()
        .and_then(|result| result.as_ref().err().map(Error::code))
        .unwrap_or("not_attempted");
    let mut playback_bytes = None;
    let mut playback_body_error_code = "none";
    if let Some(Ok(OpenedAudio::Content(mut opened))) = second_open {
        if let Some(mut body) = opened.body.take() {
            let mut bytes = Vec::new();
            loop {
                match body.chunk().await {
                    Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
                    Ok(None) => {
                        playback_bytes = Some(bytes);
                        break;
                    }
                    Err(error) => {
                        playback_body_error_code = error.code();
                        break;
                    }
                }
            }
        }
    }
    let playback_closed = f.hito.playback.close().await.is_ok();
    let replicas_closed = f.hito.replicas.close().await.is_ok();
    let replication_closed = f.hito.catalog_replication.close().await.is_ok();
    f.server.shutdown().await;
    eprintln!(
        "playback_lease_provider_fault_diagnostic first_open_error_code={first_open_error_code} second_open_error_code={second_open_error_code} body_error_code={playback_body_error_code}"
    );
    assert!(
        lease.is_some(),
        "the authorized provider lease must be created"
    );
    assert!(
        first_open_was_upstream,
        "a persistent provider failure must reject the first explicit open"
    );
    assert!(
        lease_status_preserved,
        "a temporary provider open failure must preserve lease status"
    );
    assert_eq!(playback_bytes.as_deref(), Some(f.original.as_slice()));
    assert!(
        playback_closed && replicas_closed && replication_closed,
        "owned playback services must close before assertions"
    );
}
