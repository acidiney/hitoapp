use audio_fixture::AudioLibrary;
use hito_core::OpenedAudio;
use hito_core::test_support::provider_audio as audio_fixture;

#[tokio::test]
async fn a_provider_playback_grant_reads_audio_and_cannot_survive_credential_removal() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let grant = f
        .hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = grant.open(Default::default()).await.unwrap() else {
        panic!("ready audio must open");
    };
    let mut bytes = Vec::new();
    while let Some(chunk) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    assert_eq!(bytes, f.original);
    f.hito
        .plugins
        .clear_credential(&f.actor, &f.installation.id, 1)
        .await
        .unwrap();
    assert!(grant.check().await.is_err());
    assert!(grant.open(Default::default()).await.is_err());
    assert!(grant.open_file().await.is_err());
}

#[tokio::test]
async fn logout_cancels_a_provider_handshake_without_waiting_for_the_provider() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let session = f
        .hito
        .identity
        .login(hito_core::Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = f.hito.identity.authenticate(&session.token).await.unwrap();
    let grant = f
        .hito
        .playback
        .playback_source(&actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let gate = f.server.hold("v1/audio").await;
    let mut pending = tokio::spawn(async move { grant.open(Default::default()).await });
    gate.entered().await;
    f.hito.identity.logout(&session.token).await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut pending).await;
    gate.release();
    if result.is_err() {
        pending.abort();
    }
    assert!(
        matches!(result, Ok(Ok(Err(hito_core::Error::Unauthorized)))),
        "logout must cancel the pending handshake"
    );
}

#[tokio::test]
async fn refreshing_a_connection_or_its_source_list_does_not_revoke_existing_playback() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let grant = f
        .hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
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
    assert!(matches!(updated, hito_core::PluginRefresh::Updated { .. }));
    grant.check().await.unwrap();
    f.server
        .reply("v1/sources", serde_json::json!({"items":[]}))
        .await;
    assert!(
        f.hito
            .catalog
            .refresh_sources(&f.actor, &f.source.recording_id)
            .await
            .unwrap()
            .is_empty()
    );
    grant
        .check()
        .await
        .expect("source discovery is separate from granted playback");
    assert!(grant.open(Default::default()).await.is_err());
    f.hito
        .plugins
        .withdraw_service(&f.actor, &f.installation.service_id, 1)
        .await
        .unwrap();
    assert!(grant.check().await.is_err());
}

#[tokio::test]
async fn reapproving_a_withdrawn_sources_capability_does_not_revive_old_playback() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let grant = f
        .hito
        .playback
        .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
        .await
        .unwrap();
    let mut revision = 1;
    for capabilities in [
        serde_json::json!(["search"]),
        serde_json::json!(["search", "sources"]),
    ] {
        let mut manifest = serde_json::to_value(&f.installation.manifest).unwrap();
        manifest["capabilities"] = capabilities;
        f.server.reply("v1/manifest", manifest).await;
        let hito_core::PluginRefresh::ReviewRequired { digest, .. } = f
            .hito
            .plugins
            .refresh(
                &f.actor,
                &f.installation.id,
                hito_core::RefreshPlugin {
                    revision,
                    credential: Default::default(),
                    accept_manifest_digest: None,
                },
            )
            .await
            .unwrap()
        else {
            panic!("changed capabilities require review");
        };
        let hito_core::PluginRefresh::Updated { installation } = f
            .hito
            .plugins
            .refresh(
                &f.actor,
                &f.installation.id,
                hito_core::RefreshPlugin {
                    revision,
                    credential: Default::default(),
                    accept_manifest_digest: Some(digest),
                },
            )
            .await
            .unwrap()
        else {
            panic!("reviewed manifest must update");
        };
        revision = installation.revision;
    }
    assert!(
        grant.check().await.is_err(),
        "reapproval cannot revive withdrawn output authority"
    );
    assert!(
        f.hito
            .playback
            .playback_source(&f.actor, &f.source.recording_id, &f.source.id)
            .await
            .is_ok()
    );
}
