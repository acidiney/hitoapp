#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;
use hito_core::{InstallPlugin, OfferPluginService, ProviderSearch};

#[tokio::test]
async fn provider_search_uses_the_owned_connection_and_returns_validated_results() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("provider-search-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: 1,
                bearer_token: Some("search-private-token".into()),
            },
        )
        .await
        .unwrap();
    server.reply("v1/search", serde_json::json!({"items":[{"id":"provider:item/1","title":"Private result","artist":"Private artist"}],"next_cursor":"page-two"})).await;
    let results = hito
        .catalog
        .search_provider(
            &actor,
            &installed.id,
            ProviderSearch {
                installation_revision: 1,
                query: "private query".into(),
                cursor: None,
                limit: 20,
            },
        )
        .await
        .unwrap();
    assert_eq!(results.items[0].id, "provider:item/1");
    assert_eq!(results.next_cursor.as_deref(), Some("page-two"));
    assert_eq!(
        hito.catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .total,
        0,
        "search does not fill the durable catalog"
    );
    let requests = server.requests("v1/search").await;
    assert_eq!(
        requests[0].0["authorization"],
        "Bearer search-private-token"
    );
    assert!(requests[0].0.get("cookie").is_none());
    assert_eq!(requests[0].1["query"], "private query");
}

#[tokio::test]
async fn delayed_provider_search_cannot_return_after_its_connection_is_removed() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _library, _music) =
        support::library("provider-search-fence-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: 1,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    server
        .reply(
            "v1/search",
            serde_json::json!({"items":[],"next_cursor":null}),
        )
        .await;
    let gate = server.hold("v1/search").await;
    let pending = {
        let (catalog, actor, id) = (hito.catalog.clone(), actor.clone(), installed.id.clone());
        tokio::spawn(async move {
            catalog
                .search_provider(
                    &actor,
                    &id,
                    ProviderSearch {
                        installation_revision: 1,
                        query: "Private".into(),
                        cursor: None,
                        limit: 10,
                    },
                )
                .await
        })
    };
    gate.entered().await;
    hito.plugins.remove(&actor, &installed.id, 1).await.unwrap();
    gate.release();
    assert!(matches!(
        pending.await.unwrap(),
        Err(hito_core::Error::NotFound)
    ));
}

#[tokio::test]
async fn search_rejects_another_accounts_connection_and_unapproved_capabilities() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, alice, _library, _music) =
        support::library("provider-search-private-host").await;
    hito.identity
        .create_user(
            &alice,
            hito_core::Credentials {
                username: "bob".into(),
                password: "provider bob password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(hito_core::Credentials {
            username: "bob".into(),
            password: "provider bob password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    server.reply("v1/manifest", serde_json::json!({"id":"org.hito.content-fixture","name":"Content fixture","protocol_version":1,"capabilities":["sources"]})).await;
    let service = hito
        .plugins
        .offer_service(
            &alice,
            OfferPluginService {
                name: "Fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &bob,
            InstallPlugin {
                service_id: service.id,
                service_revision: 1,
                bearer_token: Some("bob-private-token".into()),
            },
        )
        .await
        .unwrap();
    for actor in [&alice, &bob] {
        assert!(
            hito.catalog
                .search_provider(
                    actor,
                    &installed.id,
                    ProviderSearch {
                        installation_revision: 1,
                        query: "Private".into(),
                        cursor: None,
                        limit: 10
                    }
                )
                .await
                .is_err()
        );
    }
    assert!(server.requests("v1/search").await.is_empty());
}
