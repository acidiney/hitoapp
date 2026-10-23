use audio_fixture::AudioLibrary;
use hito_core::test_support::provider_audio as audio_fixture;
use hito_core::{AdoptProviderRecording, Credentials, InstallPlugin, OpenedAudio};

#[tokio::test]
async fn five_accounts_can_each_hold_playing_and_preloaded_audio_and_dropping_releases_capacity() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let mut listeners = vec![(f.actor.clone(), f.source.id.clone())];
    for index in 1..5 {
        let credentials = || Credentials {
            username: format!("listener{index}"),
            password: "listener test password".into(),
        };
        f.hito
            .identity
            .create_user(&f.actor, credentials())
            .await
            .unwrap();
        let session = f.hito.identity.login(credentials()).await.unwrap();
        let actor = f.hito.identity.authenticate(&session.token).await.unwrap();
        let installation = f
            .hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: f.installation.service_id.clone(),
                    service_revision: 1,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let recording = f
            .hito
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: 1,
                    provider_recording_id: "recording".into(),
                },
            )
            .await
            .unwrap();
        let source = f
            .hito
            .catalog
            .sources(&actor, &recording.id)
            .await
            .unwrap()
            .remove(0);
        listeners.push((actor, source.id));
    }
    let mut bodies = Vec::new();
    for (actor, source) in &listeners {
        for _ in 0..2 {
            let OpenedAudio::Content(audio) = f
                .hito
                .playback
                .open_audio(actor, source, Default::default())
                .await
                .unwrap()
            else {
                panic!("listener must open")
            };
            bodies.push(audio);
        }
        assert!(matches!(
            f.hito
                .playback
                .open_audio(actor, source, Default::default())
                .await,
            Err(hito_core::Error::Conflict(_))
        ));
    }
    assert_eq!(bodies.len(), 10);
    drop(bodies.remove(0));
    assert!(matches!(
        f.hito
            .playback
            .open_audio(&f.actor, &f.source.id, Default::default())
            .await
            .unwrap(),
        OpenedAudio::Content(_)
    ));
    assert_eq!(f.server.requests("v1/audio").await.len(), 11);
}
