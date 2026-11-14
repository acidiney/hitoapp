use super::fixture::content;
use axum::http::StatusCode;
use hito_core::{
    AudioRequest, BeginPlaybackLease, ByteRange, Error, OpenedAudio,
    test_support::{hang_guard::hang_guard, provider_audio::{AudioLibrary, headers}},
};
use std::time::Duration;

#[tokio::test]
async fn provider_lease_reads_a_prefix_before_playback_and_reuses_its_response() {
    let prefix_size = 128;
    let (f, held_tail, requests_before_lease) = hang_guard(
        "owned provider audio setup",
        async {
            let f = AudioLibrary::start().await;
            f.ready().await;
            let held_tail = f.server.hold_audio_tail(prefix_size);
            let requests_before_lease = f.server.requests("v1/audio").await.len();
            (f, held_tail, requests_before_lease)
        },
    )
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
        .await;

    let warm_started = if let Ok(lease) = &lease {
        tokio::time::timeout(Duration::from_secs(2), held_tail.entered())
            .await
            .is_ok()
    } else {
        false
    };
    held_tail.release();

    let playback = if let Ok(lease) = &lease {
        f.hito
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await
    } else {
        Err(Error::NotFound)
    };
    let audio_bytes: Result<Vec<u8>, Error> = async {
        let OpenedAudio::Content(mut audio) = playback? else {
            return Err(Error::NotFound);
        };
        let mut body = audio.body.take().ok_or(Error::NotFound)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = body.chunk().await? {
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    .await;
    let request_count = f.server.requests("v1/audio").await.len();
    let download_status = f
        .hito
        .downloads
        .status(&f.actor, &f.source.recording_id)
        .await;
    let source_bytes = std::fs::read(&f.music.join("original.wav"));
    f.hito.playback.close().await.unwrap();
    f.hito.replicas.close().await.unwrap();
    f.hito.catalog_replication.close().await.unwrap();
    f.server.shutdown().await;

    assert!(
        warm_started,
        "beginning an authorized provider lease must read its bounded audio prefix"
    );
    assert_eq!(audio_bytes.unwrap(), f.original);
    assert_eq!(request_count, requests_before_lease + 1);
    assert!(matches!(download_status, Err(Error::NotFound)));
    assert_eq!(source_bytes.unwrap(), f.original);
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
