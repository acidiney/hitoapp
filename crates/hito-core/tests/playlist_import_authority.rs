use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Actor, CoreConfig, Credentials, Error, HostDeviceId, ImportProviderPlaylist, PluginCredential,
    PluginRefresh, RefreshPlugin, Services,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
enum AuthorityChange {
    Logout,
    RemoveConnection,
    ReplaceCredential,
    WithdrawService,
}

#[tokio::test]
async fn authority_changes_while_provider_items_are_held_prevent_any_import_publication() {
    for change in [
        AuthorityChange::Logout,
        AuthorityChange::RemoveConnection,
        AuthorityChange::ReplaceCredential,
        AuthorityChange::WithdrawService,
    ] {
        let server = fixture::ProviderServer::start().await;
        let (dir, hito, observer, _shared, music) = support::library("playlist-authority").await;
        let installed = fixture::connect(&hito, &observer, &server, &["playlists"]).await;
        let other = hito_core::open(CoreConfig::new(
            dir.path().join("hito.db"),
            music,
            HostDeviceId::new("playlist-authority").unwrap(),
        ))
        .await
        .unwrap();
        let session = hito
            .identity
            .login(Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let importing_actor = hito.identity.authenticate(&session.token).await.unwrap();
        fixture::playlist(&server, "playlist/authority", "snapshot-one", vec![
            json!({"occurrence_id": "retained", "item": {"kind": "recording", "recording": {
                "id": "recording/retained", "title": "Retained recording", "artist": "Fixture artist"
            }}}),
        ]).await;
        let before = public_state(&hito, &observer).await;
        assert_eq!(before["catalog"]["total"], 0);
        assert!(before["playlists"].as_array().unwrap().is_empty());
        assert!(before["feed"]["operations"].as_array().unwrap().is_empty());
        let service_revision = other
            .plugins
            .settings(&observer)
            .await
            .unwrap()
            .services
            .into_iter()
            .find(|service| service.id == installed.service_id)
            .unwrap()
            .revision;
        let gate = server.hold("v1/playlists/items").await;
        let pending = {
            let (collection, actor, installation_id) = (
                hito.collection.clone(),
                importing_actor.clone(),
                installed.id.clone(),
            );
            let request = ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/authority-selection".into(),
            };
            tokio::spawn(async move {
                collection
                    .import_provider_playlist(&actor, &installation_id, request)
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(3), gate.entered())
            .await
            .expect("the provider item response must be held before authority changes");
        assert_eq!(server.requests("v1/playlists/detail").await.len(), 1);
        assert_eq!(server.requests("v1/playlists/items").await.len(), 1);
        tokio::time::timeout(Duration::from_secs(3), async {
            match change {
                AuthorityChange::Logout => {
                    other.identity.logout(&session.token).await.unwrap();
                    assert!(matches!(
                        other.identity.check_session(&importing_actor).await,
                        Err(Error::Unauthorized)
                    ));
                }
                AuthorityChange::RemoveConnection => {
                    other
                        .plugins
                        .remove(&observer, &installed.id, installed.revision)
                        .await
                        .unwrap();
                    assert!(
                        other
                            .plugins
                            .settings(&observer)
                            .await
                            .unwrap()
                            .installations
                            .is_empty()
                    );
                }
                AuthorityChange::ReplaceCredential => {
                    let refreshed = other
                        .plugins
                        .refresh(
                            &observer,
                            &installed.id,
                            RefreshPlugin {
                                revision: installed.revision,
                                accept_manifest_digest: None,
                                credential: PluginCredential::Replace {
                                    bearer_token: "owned-fixture-replacement".into(),
                                },
                            },
                        )
                        .await
                        .unwrap();
                    let PluginRefresh::Updated { installation } = refreshed else {
                        panic!("the unchanged manifest must accept the credential replacement")
                    };
                    assert_eq!(installation.id, installed.id);
                    assert!(installation.revision > installed.revision);
                    assert!(installation.credential_set);
                }
                AuthorityChange::WithdrawService => {
                    other
                        .plugins
                        .withdraw_service(&observer, &installed.service_id, service_revision)
                        .await
                        .unwrap();
                    assert!(
                        other
                            .plugins
                            .settings(&observer)
                            .await
                            .unwrap()
                            .services
                            .is_empty()
                    );
                }
            }
        })
        .await
        .expect("authority changes must commit while the provider response remains held");
        assert_eq!(public_state(&hito, &observer).await, before, "{change:?}");
        gate.release();
        let result = tokio::time::timeout(Duration::from_secs(3), pending)
            .await
            .expect("stale imports must finish after the provider response is released")
            .unwrap();
        match (change, result) {
            (AuthorityChange::Logout, Err(Error::Unauthorized))
            | (
                AuthorityChange::RemoveConnection | AuthorityChange::WithdrawService,
                Err(Error::NotFound),
            )
            | (AuthorityChange::ReplaceCredential, Err(Error::Conflict(_))) => {}
            _ => panic!("{change:?} must reject the stale import with its authority error"),
        }
        assert_eq!(public_state(&hito, &observer).await, before, "{change:?}");
        assert_eq!(server.requests("v1/playlists/items").await.len(), 1);
    }
}

async fn public_state(hito: &Services, actor: &Actor) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap()
    })
}
