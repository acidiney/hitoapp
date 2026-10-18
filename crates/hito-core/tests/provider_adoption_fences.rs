#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;
use hito_core::{AdoptProviderRecording, InstallPlugin, OfferPluginService};

async fn connect(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
    server: &fixture::ProviderServer,
) -> hito_core::PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: 1,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}
fn request(id: &str) -> AdoptProviderRecording {
    AdoptProviderRecording {
        installation_revision: 1,
        provider_recording_id: id.into(),
    }
}

#[tokio::test]
async fn removing_a_provider_release_removes_its_obsolete_artist_association() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-release-removal").await;
    let installed = connect(&hito, &actor, &server).await;
    recording(&server, "first", "First track").await;
    server.reply("v1/recordings/detail", serde_json::json!({"id":"first","title":"First track","artist":"Before","release":{"id":"release","title":"Release","artist":"Before"}})).await;
    hito.catalog
        .adopt_provider(&actor, &installed.id, request("first"))
        .await
        .unwrap();
    assert_eq!(
        hito.catalog
            .artists(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
    server
        .reply(
            "v1/recordings/detail",
            serde_json::json!({"id":"first","title":"First track","artist":"After"}),
        )
        .await;
    hito.catalog
        .adopt_provider(&actor, &installed.id, request("first"))
        .await
        .unwrap();
    let artists = hito
        .catalog
        .artists(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "After");
    assert_eq!(artists.items[0].release_count, 0);
    let detail = hito
        .catalog
        .artist(&actor, &artists.items[0].id, Default::default())
        .await
        .unwrap();
    assert_eq!(detail.recordings.total, 1);
    assert_eq!(detail.recordings.items[0].artist, "After");
    assert_eq!(
        hito.catalog
            .releases(&actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
}
async fn recording(server: &fixture::ProviderServer, id: &str, title: &str) {
    server
        .reply(
            "v1/recordings/detail",
            serde_json::json!({"id":id,"title":title,"artist":"Provider artist"}),
        )
        .await;
    server
        .reply(
            "v1/sources",
            serde_json::json!({"items":[{"id":"same-source","label":"Original","ready":true}]}),
        )
        .await;
}

#[tokio::test]
async fn malformed_detail_or_reparented_source_cannot_publish_partial_catalog_changes() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-adoption-invalid").await;
    let installed = connect(&hito, &actor, &server).await;
    recording(&server, "first", "First track").await;
    let first = hito
        .catalog
        .adopt_provider(&actor, &installed.id, request("first"))
        .await
        .unwrap();
    assert!(
        hito.catalog
            .adopt_provider(&actor, &installed.id, request("different"))
            .await
            .is_err()
    );
    recording(&server, "second", "Second track").await;
    assert!(
        hito.catalog
            .adopt_provider(&actor, &installed.id, request("second"))
            .await
            .is_err()
    );
    let page = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, first.id);
    assert!(page.items[0].available);
    server.reply("v1/sources", serde_json::json!({"items":[{"id":"same-source","label":"Original","ready":true},{"id":"same-source","label":"Duplicate","ready":true}]})).await;
    assert!(
        hito.catalog
            .adopt_provider(&actor, &installed.id, request("second"))
            .await
            .is_err()
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
}

#[tokio::test]
async fn removal_during_source_discovery_prevents_adoption_and_does_not_create_a_library() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-adoption-fence").await;
    let installed = connect(&hito, &actor, &server).await;
    recording(&server, "first", "First track").await;
    let gate = server.hold("v1/sources").await;
    let pending = {
        let (catalog, actor, id) = (hito.catalog.clone(), actor.clone(), installed.id.clone());
        tokio::spawn(async move { catalog.adopt_provider(&actor, &id, request("first")).await })
    };
    gate.entered().await;
    hito.plugins.remove(&actor, &installed.id, 1).await.unwrap();
    gate.release();
    assert!(matches!(
        pending.await.unwrap(),
        Err(hito_core::Error::NotFound)
    ));
    assert_eq!(hito.catalog.libraries(&actor).await.unwrap().len(), 1);
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn accepted_capability_changes_and_service_withdrawal_control_cached_readiness() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-live-authority").await;
    let installed = connect(&hito, &actor, &server).await;
    recording(&server, "first", "First track").await;
    let track = hito
        .catalog
        .adopt_provider(&actor, &installed.id, request("first"))
        .await
        .unwrap();
    let source = hito
        .catalog
        .sources(&actor, &track.id)
        .await
        .unwrap()
        .remove(0);
    let mut revision = 1;
    for (capabilities, ready) in [(vec!["search"], false), (vec!["search", "sources"], true)] {
        server.reply("v1/manifest", serde_json::json!({"id":"org.hito.content-fixture","name":"Content fixture","protocol_version":1,"capabilities":capabilities})).await;
        let review = hito
            .plugins
            .refresh(
                &actor,
                &installed.id,
                hito_core::RefreshPlugin {
                    revision,
                    accept_manifest_digest: None,
                    credential: Default::default(),
                },
            )
            .await
            .unwrap();
        let hito_core::PluginRefresh::ReviewRequired { digest, .. } = review else {
            panic!("capability change needs review")
        };
        let accepted = hito
            .plugins
            .refresh(
                &actor,
                &installed.id,
                hito_core::RefreshPlugin {
                    revision,
                    accept_manifest_digest: Some(digest),
                    credential: Default::default(),
                },
            )
            .await
            .unwrap();
        let hito_core::PluginRefresh::Updated { installation } = accepted else {
            panic!("reviewed manifest must update")
        };
        revision = installation.revision;
        let current = hito
            .catalog
            .sources(&actor, &track.id)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(current.id, source.id);
        assert_eq!(current.ready, ready);
    }
    hito.plugins
        .withdraw_service(&actor, &installed.service_id, 1)
        .await
        .unwrap();
    assert!(!hito.catalog.sources(&actor, &track.id).await.unwrap()[0].ready);
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        1
    );
}
