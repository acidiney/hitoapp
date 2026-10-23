use super::*;

#[tokio::test]
async fn zero_file_id_is_explicitly_minted_and_opened_without_selecting_an_archive() {
    let media = MediaFixture::start(wave()).await;
    let mut configured = binding();
    configured.file_id = 0;
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&observed(true, true, "album/owned.wav")).unwrap();
    metadata["data"]["files"][1]["id"] = json!(0);
    let metadata = serde_json::to_vec(&metadata).unwrap();
    let fixture = Fixture::start(vec![metadata.clone(), acknowledgment(), metadata]).await;
    let client = TorBoxClient::with_test_tls_media(
        TOKEN.into(),
        fixture.ca_der(),
        fixture.address(),
        media.ca_der(),
        media.address(),
    )
    .unwrap();
    let capture = Capture::default();
    let candidate = client
        .prepare_bound_file(&configured)
        .with_subscriber(capture.dispatch())
        .await
        .unwrap();
    let mut opened = client
        .open_bound_audio(
            candidate,
            &MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
        )
        .with_subscriber(capture.dispatch())
        .await
        .unwrap();
    assert!(opened.binding == configured);
    let mut bytes = Vec::new();
    while let Some(chunk) = opened.next_chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert!(bytes == wave());
    assert_pcm(bytes);
    let requests = fixture.close().await;
    assert_eq!(requests.len(), 3);
    assert!(
        requests[1]
            .target
            .contains("&file_id=0&zip_link=false&redirect=false&append_name=false")
    );
    assert_eq!(media.close().await.len(), 1);
    capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_file_preparation", "candidate"),
            ("torrent_observation", "provider_completed"),
            ("bound_audio_opening", "complete"),
            ("bound_audio_opening", "complete"),
        ],
        &[TOKEN],
    );
}

#[tokio::test]
async fn cancelled_media_qualification_closes_peer_and_releases_admission_before_recovery() {
    let (mut held, closed) = scripted_media::Reply::held(wave());
    held.held_bytes = 0;
    let media =
        scripted_media::ScriptedMedia::start(vec![held, scripted_media::Reply::full(wave())]).await;
    let fixture = Fixture::start(vec![
        observed(true, true, "album/owned.wav"),
        acknowledgment(),
        observed(true, true, "album/owned.wav"),
        observed(true, true, "album/owned.wav"),
        acknowledgment(),
        observed(true, true, "album/owned.wav"),
    ])
    .await;
    let client = TorBoxClient::with_test_tls_media(
        TOKEN.into(),
        fixture.ca_der(),
        fixture.address(),
        media.ca_der(),
        media.address(),
    )
    .unwrap();
    let candidate = client.prepare_bound_file(&binding()).await.unwrap();
    let opening = client.clone();
    let capture = Capture::default();
    let dispatch = capture.dispatch();
    let task = tokio::spawn(async move {
        opening
            .open_bound_audio(
                candidate,
                &MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
            )
            .with_subscriber(dispatch)
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), media.wait_for_requests(1))
        .await
        .unwrap();
    task.abort();
    assert!(matches!(task.await,Err(error) if error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .unwrap()
        .unwrap();
    capture.assert_operations(
        &[
            ("torrent_observation", "provider_completed"),
            ("bound_audio_opening", "cancelled"),
        ],
        &[TOKEN],
    );
    let candidate = client.prepare_bound_file(&binding()).await.unwrap();
    let mut opened = client
        .open_bound_audio(
            candidate,
            &MediaAllowlist::new(vec![format!("https://{MEDIA_HOST}")]).unwrap(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = opened.next_chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert!(bytes == wave());
    assert_pcm(bytes);
    drop(client);
    assert_eq!(fixture.close().await.len(), 6);
    assert_eq!(media.close().await, 2);
}
