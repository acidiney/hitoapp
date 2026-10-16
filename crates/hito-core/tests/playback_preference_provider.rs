#[path = "fixtures/provider_audio.rs"]
mod audio_fixture;
#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

#[tokio::test]
async fn preferred_provider_uses_the_current_snapshot_and_respects_withdrawal() {
    let f = audio_fixture::AudioLibrary::start().await;
    f.ready().await;
    let grant = f
        .hito
        .catalog
        .preferred_playback_source(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert!(!grant.is_local());
    let opened = grant.open(Default::default()).await.unwrap();
    let hito_core::OpenedAudio::Content(mut content) = opened else {
        panic!("expected owned audio");
    };
    let mut received = Vec::new();
    let mut body = content.body.take().unwrap();
    while let Some(chunk) = body.chunk().await.unwrap() {
        received.extend_from_slice(&chunk);
    }
    assert_eq!(received, f.original);
    drop(body);
    f.server
        .reply("v1/sources", serde_json::json!({"items":[]}))
        .await;
    f.hito
        .catalog
        .refresh_sources(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert!(matches!(
        f.hito
            .catalog
            .preferred_playback_source(&f.actor, &f.source.recording_id)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    f.server.reply("v1/sources", serde_json::json!({"items":[{"id":"audio:/1?private=value&other=bad","label":"Original","ready":true}]})).await;
    f.hito
        .catalog
        .refresh_sources(&f.actor, &f.source.recording_id)
        .await
        .unwrap();
    assert!(
        f.hito
            .catalog
            .preferred_playback_source(&f.actor, &f.source.recording_id)
            .await
            .is_ok()
    );
    f.hito
        .plugins
        .withdraw_service(&f.actor, &f.installation.service_id, 1)
        .await
        .unwrap();
    assert!(matches!(
        f.hito
            .catalog
            .preferred_playback_source(&f.actor, &f.source.recording_id)
            .await,
        Err(hito_core::Error::NotFound)
    ));
}
