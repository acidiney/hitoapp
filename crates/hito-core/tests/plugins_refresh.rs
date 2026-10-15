#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{Error, InstallPlugin, OfferPluginService, PluginRefresh, RefreshPlugin};

#[tokio::test]
async fn manifest_changes_require_review_and_cannot_replace_a_pinned_plugin_identity() {
    let server = fixture::PluginServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("plugin-refresh-host").await;
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
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let PluginRefresh::Updated {
        installation: checked,
    } = hito
        .plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: installed.revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("Unchanged manifest required review");
    };
    assert_eq!(checked.id, installed.id);
    assert_eq!(checked.revision, installed.revision + 1);
    let mut changed = fixture::manifest();
    changed["name"] = "Updated provider".into();
    server.reply(changed.clone()).await;
    let PluginRefresh::ReviewRequired { manifest, digest } = hito
        .plugins
        .refresh(
            &actor,
            &checked.id,
            RefreshPlugin {
                revision: checked.revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("Changed manifest was accepted without review");
    };
    assert_eq!(manifest.name, "Updated provider");
    assert_eq!(
        hito.plugins.settings(&actor).await.unwrap().installations[0]
            .manifest
            .name,
        "Fixture provider"
    );
    assert_eq!(
        hito.plugins.settings(&actor).await.unwrap().installations[0].revision,
        checked.revision
    );
    let PluginRefresh::Updated {
        installation: accepted,
    } = hito
        .plugins
        .refresh(
            &actor,
            &checked.id,
            RefreshPlugin {
                revision: checked.revision,
                accept_manifest_digest: Some(digest),
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("Reviewed manifest was not accepted");
    };
    assert_eq!(accepted.id, installed.id);
    assert_eq!(accepted.manifest.name, "Updated provider");
    changed["id"] = "org.other.provider".into();
    server.reply(changed).await;
    assert!(matches!(
        hito.plugins
            .refresh(
                &actor,
                &accepted.id,
                RefreshPlugin {
                    revision: accepted.revision,
                    accept_manifest_digest: None,
                    credential: Default::default(),
                }
            )
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        hito.plugins.settings(&actor).await.unwrap().installations[0]
            .manifest
            .id,
        "org.hito.test"
    );
}
