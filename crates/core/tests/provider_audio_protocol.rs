use audio_fixture::AudioLibrary;
use axum::http::StatusCode;
use hito_core::test_support::content_service as fixture;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::{AudioRequest, ByteRange, OpenedAudio};

#[tokio::test]
async fn malformed_audio_handshakes_are_rejected_without_following_redirects_or_leaking_slots() {
    let f = AudioLibrary::start().await;
    let target = fixture::ProviderServer::start().await;
    let mut headers = audio_fixture::headers();
    headers.insert(
        "location",
        format!("{}v1/audio?source_id=redirect", target.url)
            .parse()
            .unwrap(),
    );
    f.server
        .audio(StatusCode::FOUND, headers, f.original.clone())
        .await;
    assert!(
        f.hito
            .playback
            .open_audio(&f.actor, &f.source.id, Default::default())
            .await
            .is_err()
    );
    assert!(target.requests("v1/audio").await.is_empty());
    for (status, headers, length, range) in [
        (StatusCode::UNAUTHORIZED, vec![], 20, None),
        (
            StatusCode::OK,
            vec![("content-type", "text/html")],
            20,
            None,
        ),
        (StatusCode::OK, vec![("content-encoding", "gzip")], 20, None),
        (
            StatusCode::OK,
            vec![("content-range", "bytes 44-63/16044")],
            20,
            None,
        ),
        (
            StatusCode::PARTIAL_CONTENT,
            vec![],
            20,
            Some(ByteRange::From {
                first: 44,
                last: Some(63),
            }),
        ),
        (
            StatusCode::PARTIAL_CONTENT,
            vec![("content-range", "bytes 45-64/16044")],
            20,
            Some(ByteRange::From {
                first: 44,
                last: Some(63),
            }),
        ),
        (
            StatusCode::PARTIAL_CONTENT,
            vec![("content-range", "bytes 44-63/16044")],
            19,
            Some(ByteRange::From {
                first: 44,
                last: Some(63),
            }),
        ),
        (
            StatusCode::PARTIAL_CONTENT,
            vec![("content-range", "bytes 44-63/50")],
            20,
            Some(ByteRange::From {
                first: 44,
                last: Some(63),
            }),
        ),
        (
            StatusCode::PARTIAL_CONTENT,
            vec![("content-range", "bytes 44-63/*")],
            20,
            Some(ByteRange::Suffix { length: 20 }),
        ),
        (
            StatusCode::PARTIAL_CONTENT,
            vec![("content-range", "bytes 44-63/16044")],
            20,
            None,
        ),
    ] {
        let mut response = audio_fixture::headers();
        for (name, value) in headers {
            response.insert(name, value.parse().unwrap());
        }
        f.server.audio(status, response, vec![0; length]).await;
        assert!(matches!(
            f.hito
                .playback
                .open_audio(&f.actor, &f.source.id, AudioRequest { head: false, range })
                .await,
            Err(hito_core::Error::Upstream(_))
        ));
    }
    f.ready().await;
    assert!(matches!(
        f.hito
            .playback
            .open_audio(&f.actor, &f.source.id, Default::default())
            .await
            .unwrap(),
        OpenedAudio::Content(_)
    ));
}

#[tokio::test]
async fn audio_requires_the_owning_account_before_contacting_the_provider() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    f.hito
        .identity
        .create_user(
            &f.actor,
            hito_core::Credentials {
                username: "bob".into(),
                password: "provider bob password".into(),
            },
        )
        .await
        .unwrap();
    let session = f
        .hito
        .identity
        .login(hito_core::Credentials {
            username: "bob".into(),
            password: "provider bob password".into(),
        })
        .await
        .unwrap();
    let bob = f.hito.identity.authenticate(&session.token).await.unwrap();
    assert!(matches!(
        f.hito
            .playback
            .open_audio(&bob, &f.source.id, Default::default())
            .await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(f.server.requests("v1/audio").await.is_empty());
}
