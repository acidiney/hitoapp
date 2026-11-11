use hito_core::test_support::playback::LocalPlayback;
use hito_core::{AudioRequest, BeginPlaybackLease, ByteRange, OpenedAudio};

#[tokio::test]
async fn lease_streams_the_exact_authorized_file_and_independent_ranges() {
    let f = LocalPlayback::start().await;
    let lease = f
        .services
        .playback
        .begin_lease(
            &f.actor,
            BeginPlaybackLease {
                recording_id: f.recording.clone(),
                source_id: f.source.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .unwrap(),
        lease
    );
    let opened = f
        .services
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: false,
                range: Some(ByteRange::From {
                    first: 7,
                    last: Some(31),
                }),
            },
        )
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = opened else {
        panic!("expected range")
    };
    assert_eq!(audio.range, Some(7..=31));
    assert_eq!(audio.total_length, Some(f.original.len() as u64));
    let mut actual = Vec::new();
    let mut body = audio.body.take().unwrap();
    while let Some(chunk) = body.chunk().await.unwrap() {
        actual.extend_from_slice(&chunk);
    }
    assert_eq!(actual, f.original[7..=31]);
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    assert!(
        f.services
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await
            .is_err()
    );
    f.services.playback.close().await.unwrap();
    f.services.replicas.close().await.unwrap();
    f.services.catalog_replication.close().await.unwrap();
}

#[tokio::test]
async fn a_lease_handle_never_authorizes_another_session_even_after_release() {
    let f = LocalPlayback::start().await;
    let login = f
        .services
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let other = f
        .services
        .identity
        .authenticate(&login.token)
        .await
        .unwrap();
    let lease = f
        .services
        .playback
        .begin_lease(
            &f.actor,
            BeginPlaybackLease {
                recording_id: f.recording.clone(),
                source_id: f.source.clone(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.services.playback.lease_status(&other, lease.id).await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(matches!(
        f.services
            .playback
            .open_lease_audio(&other, lease.id, AudioRequest::default())
            .await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(matches!(
        f.services.playback.release_lease(&other, lease.id).await,
        Err(hito_core::Error::NotFound)
    ));
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    assert!(matches!(
        f.services.playback.release_lease(&other, lease.id).await,
        Err(hito_core::Error::NotFound)
    ));
    f.services.playback.close().await.unwrap();
    f.services.replicas.close().await.unwrap();
    f.services.catalog_replication.close().await.unwrap();
}
