#[path = "fixtures/source_refresh.rs"]
mod fixture;
mod support;

use fixture::Fixture;
use serde_json::json;

#[tokio::test]
async fn invalid_source_pages_preserve_the_last_accepted_sources_and_metadata() {
    let f = Fixture::start(
        json!({"items":[{"id":"original/audio", "label":"Original", "ready":true}]}),
    )
    .await;
    let before = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    for invalid in [
        json!({"items":[{"id":"duplicate", "label":"First", "ready":true}, {"id":"duplicate", "label":"Second", "ready":true}]}),
        json!({"items":(0..17).map(|n| json!({"id":n.to_string(),"label":"Too many","ready":true})).collect::<Vec<_>>()}),
        json!({"items":[{"id":"new/audio","label":"Malformed","ready":"yes"}]}),
        json!({"invalid":"not a complete source list"}),
    ] {
        f.server.reply("v1/sources", invalid).await;
        assert!(matches!(
            f.hito
                .catalog
                .refresh_sources(&f.actor, &f.recording.id)
                .await,
            Err(hito_core::Error::Upstream(_))
        ));
        let after = f
            .hito
            .catalog
            .sources(&f.actor, &f.recording.id)
            .await
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, before[0].id);
        assert!(after[0].ready);
    }
    f.server.reply("v1/sources", json!({"items":[]})).await;
    assert!(
        f.hito
            .catalog
            .refresh_sources(&f.actor, &f.recording.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !f.hito
            .catalog
            .recordings(&f.actor, Default::default())
            .await
            .unwrap()
            .items[0]
            .available
    );
    assert_eq!(
        f.hito
            .catalog
            .recording_metadata(&f.actor, &f.recording.id, Default::default())
            .await
            .unwrap()
            .observations
            .items
            .len(),
        1
    );
}

#[tokio::test]
async fn another_account_cannot_initiate_refresh_and_removal_fences_a_pending_response() {
    let f = Fixture::start(
        json!({"items":[{"id":"original/audio", "label":"Original", "ready":true}]}),
    )
    .await;
    let credentials = || hito_core::Credentials {
        username: "other".into(),
        password: "other refresh password".into(),
    };
    f.hito
        .identity
        .create_user(&f.actor, credentials())
        .await
        .unwrap();
    let session = f.hito.identity.login(credentials()).await.unwrap();
    let other = f.hito.identity.authenticate(&session.token).await.unwrap();
    let count = f.server.requests("v1/sources").await.len();
    assert!(matches!(
        f.hito
            .catalog
            .refresh_sources(&other, &f.recording.id)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    assert_eq!(f.server.requests("v1/sources").await.len(), count);
    let gate = f.server.hold("v1/sources").await;
    let pending = {
        let catalog = f.hito.catalog.clone();
        let actor = f.actor.clone();
        let id = f.recording.id.clone();
        tokio::spawn(async move { catalog.refresh_sources(&actor, &id).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), gate.entered())
        .await
        .unwrap();
    f.hito
        .plugins
        .remove(&f.actor, &f.installation.id, f.installation.revision)
        .await
        .unwrap();
    gate.release();
    assert!(matches!(
        pending.await.unwrap(),
        Err(hito_core::Error::NotFound)
    ));
    let after = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert!(!after[0].ready);
}

#[tokio::test]
async fn reused_source_identity_rolls_back_the_complete_refresh() {
    let f = Fixture::start(
        json!({"items":[{"id":"original/audio", "label":"Original", "ready":true}]}),
    )
    .await;
    let before = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    f.server
        .reply(
            "v1/recordings/detail",
            json!({"id":"other/item","title":"Other recording","artist":"Other artist"}),
        )
        .await;
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"other/audio","label":"Other","ready":true}]}),
        )
        .await;
    let other = f
        .hito
        .catalog
        .adopt_provider(
            &f.actor,
            &f.installation.id,
            hito_core::AdoptProviderRecording {
                installation_revision: f.installation.revision,
                provider_recording_id: "other/item".into(),
            },
        )
        .await
        .unwrap();
    f.server.reply("v1/sources", json!({"items":[{"id":"new/audio","label":"Partial","ready":true},{"id":"other/audio","label":"Reused","ready":true}]})).await;
    assert!(matches!(
        f.hito
            .catalog
            .refresh_sources(&f.actor, &f.recording.id)
            .await,
        Err(hito_core::Error::Upstream(_))
    ));
    let after = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[0].id);
    assert!(after[0].ready);
    assert!(f.hito.catalog.sources(&f.actor, &other.id).await.unwrap()[0].ready);
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"new/audio","label":"Valid","ready":true}]}),
        )
        .await;
    assert!(
        f.hito
            .catalog
            .refresh_sources(&f.actor, &f.recording.id)
            .await
            .unwrap()[0]
            .ready
    );
}
