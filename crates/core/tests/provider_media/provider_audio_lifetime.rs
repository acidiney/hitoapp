use audio_fixture::AudioLibrary;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::{AudioRequest, OpenedAudio};
use std::time::Duration;

#[tokio::test]
async fn removing_a_connection_cancels_a_pending_audio_handshake() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("v1/audio").await;
    let pending = {
        let (playback, actor, source) = (
            f.hito.playback.clone(),
            f.actor.clone(),
            f.source.id.clone(),
        );
        tokio::spawn(async move {
            playback
                .open_audio(&actor, &source, AudioRequest::default())
                .await
        })
    };
    gate.entered().await;
    f.hito
        .plugins
        .remove(&f.actor, &f.installation.id, 1)
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), pending).await;
    gate.release();
    assert!(matches!(
        result
            .expect("removal must cancel the pending handshake")
            .unwrap(),
        Err(hito_core::Error::NotFound)
    ));
}

#[tokio::test]
async fn withdrawing_a_service_cancels_an_in_progress_audio_read() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-body").await;
    let OpenedAudio::Content(mut audio) = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap()
    else {
        panic!("audio must open")
    };
    let mut received = 0;
    while received < f.original.len() {
        received += audio
            .body
            .as_mut()
            .unwrap()
            .chunk()
            .await
            .unwrap()
            .unwrap()
            .len();
    }
    let mut pending = tokio::spawn(async move { audio.body.as_mut().unwrap().chunk().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut pending)
            .await
            .is_err()
    );
    f.hito
        .plugins
        .withdraw_service(&f.actor, &f.installation.service_id, 1)
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .expect("withdrawal must cancel the read")
            .unwrap(),
        Err(hito_core::Error::NotFound)
    ));
    gate.release();
}

#[tokio::test]
async fn clearing_credentials_in_another_service_cancels_unpolled_bodies_and_releases_slots() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-body").await;
    let OpenedAudio::Content(mut first) = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("ready source must open")
    };
    let OpenedAudio::Content(mut second) = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("second stream must open")
    };
    gate.entered().await;
    assert!(matches!(
        f.hito
            .playback
            .open_audio(&f.actor, &f.source.id, AudioRequest::default())
            .await,
        Err(hito_core::Error::Conflict(_))
    ));
    let other = hito_core::open(hito_core::CoreConfig::new(
        f.directory.path().join("hito.db"),
        f.music.clone(),
        hito_core::HostDeviceId::new("provider-audio").unwrap(),
    ))
    .await
    .unwrap();
    other
        .plugins
        .clear_credential(&f.actor, &f.installation.id, 1)
        .await
        .unwrap();
    let reopened = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match f
                .hito
                .playback
                .open_audio(&f.actor, &f.source.id, AudioRequest::default())
                .await
            {
                Err(hito_core::Error::Conflict(_)) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                result => return result,
            }
        }
    })
    .await
    .expect("revocation must release unread audio slots")
    .unwrap();
    assert!(matches!(reopened, OpenedAudio::Content(_)));
    assert!(first.body.as_mut().unwrap().chunk().await.is_err());
    assert!(second.body.as_mut().unwrap().chunk().await.is_err());
}

#[tokio::test]
async fn connection_checks_and_cached_readiness_changes_do_not_interrupt_a_healthy_body() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let gate = f.server.hold("audio-body").await;
    let OpenedAudio::Content(mut audio) = f
        .hito
        .playback
        .open_audio(&f.actor, &f.source.id, Default::default())
        .await
        .unwrap()
    else {
        panic!("ready source must open")
    };
    let updated = f
        .hito
        .plugins
        .refresh(
            &f.actor,
            &f.installation.id,
            hito_core::RefreshPlugin {
                revision: 1,
                credential: Default::default(),
                accept_manifest_digest: None,
            },
        )
        .await
        .unwrap();
    let hito_core::PluginRefresh::Updated { installation } = updated else {
        panic!("unchanged manifest needs no review")
    };
    let mut bytes = Vec::new();
    while bytes.len() < f.original.len() {
        bytes.extend_from_slice(&audio.body.as_mut().unwrap().chunk().await.unwrap().unwrap());
    }
    assert_eq!(bytes, f.original);
    f.server
        .reply("v1/sources", serde_json::json!({"items":[]}))
        .await;
    f.hito
        .catalog
        .adopt_provider(
            &f.actor,
            &f.installation.id,
            hito_core::AdoptProviderRecording {
                installation_revision: installation.revision,
                provider_recording_id: "recording".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        f.hito
            .catalog
            .sources(&f.actor, &f.source.recording_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        f.hito
            .playback
            .open_audio(&f.actor, &f.source.id, Default::default())
            .await
            .is_err()
    );
    gate.release();
    assert!(
        audio
            .body
            .as_mut()
            .unwrap()
            .chunk()
            .await
            .unwrap()
            .is_none()
    );
}
