use super::*;

#[tokio::test]
async fn active_media_bodies_hold_shared_admission_and_drop_closes_peer_before_recovery() {
    let (first, first_closed) = scripted_media::Reply::held(wave());
    let (second, second_closed) = scripted_media::Reply::held(wave());
    let media = scripted_media::ScriptedMedia::start(vec![
        first,
        second,
        scripted_media::Reply::full(wave()),
    ])
    .await;
    let mut replies = Vec::new();
    for _ in 0..3 {
        replies.push(Some(observed(true, true, "album/owned.wav")));
        replies.push(Some(acknowledgment()));
    }
    replies.extend([
        Some(observed(true, true, "album/owned.wav")),
        Some(observed(true, true, "album/owned.wav")),
        Some(observed(true, true, "album/owned.wav")),
        Some(acknowledgment()),
        Some(observed(true, true, "album/owned.wav")),
    ]);
    let fixture = provider_fixture::ProviderFixture::start(replies).await;
    let client = TorBoxClient::with_test_tls_media(
        TOKEN.into(),
        fixture.ca_der(),
        fixture.address(),
        media.ca_der(),
        media.address(),
    )
    .unwrap();
    let a = client.prepare_bound_file(&binding()).await.unwrap();
    let b = client.prepare_bound_file(&binding()).await.unwrap();
    let excess = client.prepare_bound_file(&binding()).await.unwrap();
    let allowlist = MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap();
    let first_capture = Capture::default();
    let second_capture = Capture::default();
    let first = client
        .open_bound_audio(a, &allowlist)
        .with_subscriber(first_capture.dispatch())
        .await
        .unwrap();
    let second = client
        .clone()
        .open_bound_audio(b, &allowlist)
        .with_subscriber(second_capture.dispatch())
        .await
        .unwrap();
    let busy = Capture::default();
    assert!(matches!(
        client
            .open_bound_audio(excess, &allowlist)
            .with_subscriber(busy.dispatch())
            .await,
        Err(Error::Busy)
    ));
    assert_eq!(media.requests(), 2);
    drop(first);
    tokio::time::timeout(Duration::from_secs(2), first_closed)
        .await
        .unwrap()
        .unwrap();
    first_capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_audio_opening", "complete"),
            ("bound_audio_opening", "cancelled"),
        ],
        &[TOKEN],
    );
    busy.assert_operations(&[("bound_audio_opening", "busy")], &[TOKEN]);
    let candidate = client.prepare_bound_file(&binding()).await.unwrap();
    let mut recovered = client
        .open_bound_audio(candidate, &allowlist)
        .await
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = recovered.next_chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert!(bytes == wave());
    drop(second);
    tokio::time::timeout(Duration::from_secs(2), second_closed)
        .await
        .unwrap()
        .unwrap();
    second_capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_audio_opening", "complete"),
            ("bound_audio_opening", "cancelled"),
        ],
        &[TOKEN],
    );
    drop(client);
    assert_eq!(fixture.close().await, 11);
    assert_eq!(media.close().await, 3);
}
