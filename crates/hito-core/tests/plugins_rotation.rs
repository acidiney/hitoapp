use hito_core::test_support::plugin_service as fixture;
use hito_core::test_support::support;
use hito_core::{
    InstallPlugin, OfferPluginService, PluginCredential, PluginRefresh, RefreshPlugin,
};

#[tokio::test]
async fn failed_and_unreviewed_changes_do_not_replace_accepted_credentials() {
    let server = fixture::PluginServer::start().await;
    server.require_bearer("accepted-token").await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-rotation-host").await;
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
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: Some("accepted-token".into()),
            },
        )
        .await
        .unwrap();
    assert!(
        hito.plugins
            .refresh(
                &actor,
                &installed.id,
                RefreshPlugin {
                    revision: 1,
                    accept_manifest_digest: None,
                    credential: PluginCredential::Replace {
                        bearer_token: "wrong-token".into()
                    }
                }
            )
            .await
            .is_err()
    );
    server.require_bearer("new-token").await;
    let mut changed = fixture::manifest();
    changed["name"] = "New name".into();
    server.reply(changed.clone()).await;
    let PluginRefresh::ReviewRequired { digest, .. } = hito
        .plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: 1,
                accept_manifest_digest: None,
                credential: PluginCredential::Replace {
                    bearer_token: "new-token".into(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("Expected review");
    };
    assert_eq!(
        hito.plugins.settings(&actor).await.unwrap().installations[0].revision,
        1
    );
    server.require_bearer("accepted-token").await;
    server.reply(fixture::manifest()).await;
    let PluginRefresh::Updated { installation } = hito
        .plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: 1,
                accept_manifest_digest: None,
                credential: PluginCredential::Keep,
            },
        )
        .await
        .unwrap()
    else {
        panic!("Accepted credentials were not retained");
    };
    server.require_bearer("new-token").await;
    server.reply(changed).await;
    let PluginRefresh::Updated { installation } = hito
        .plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: Some(digest),
                credential: PluginCredential::Replace {
                    bearer_token: "new-token".into(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("Review was not accepted");
    };
    hito.plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: None,
                credential: PluginCredential::Keep,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        server.headers().await.last().unwrap()["authorization"],
        "Bearer new-token"
    );
}
