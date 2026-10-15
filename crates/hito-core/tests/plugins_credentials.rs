#[path = "fixtures/plugin_service.rs"]
mod fixture;
mod support;

use hito_core::{
    CoreConfig, HostDeviceId, InstallPlugin, OfferPluginService, PluginCredential, RefreshPlugin,
};

#[tokio::test]
async fn personal_credentials_survive_restart_and_are_never_returned_in_settings() {
    let server = fixture::PluginServer::start().await;
    let (dir, hito, actor, _library, music) = support::library("plugin-credentials-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Private fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let token = "fixture-private-token";
    server.require_bearer(token).await;
    let installed = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: Some(token.into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        server.headers().await[0]["authorization"],
        format!("Bearer {token}")
    );
    let settings = hito.plugins.settings(&actor).await.unwrap();
    assert!(settings.installations[0].credential_set);
    assert!(!serde_json::to_string(&settings).unwrap().contains(token));
    let config = || {
        CoreConfig::new(
            dir.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("plugin-credentials-host").unwrap(),
        )
    };
    let reopened = hito_core::open(config()).await.unwrap();
    reopened
        .plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: installed.revision,
                accept_manifest_digest: None,
                credential: PluginCredential::Keep,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        server.headers().await[1]["authorization"],
        format!("Bearer {token}")
    );
    let key = std::fs::read(dir.path().join("hito.plugin-key")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join("hito.plugin-key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    for path in ["hito.db", "hito.db-wal"] {
        if let Ok(bytes) = std::fs::read(dir.path().join(path)) {
            assert!(
                !bytes
                    .windows(token.len())
                    .any(|part| part == token.as_bytes()),
                "database files must not contain plaintext tokens"
            );
        }
    }
    std::fs::remove_file(dir.path().join("hito.plugin-key")).unwrap();
    let degraded = hito_core::open(config()).await.unwrap();
    assert!(degraded.healthy().await);
    assert!(
        !degraded
            .plugins
            .settings(&actor)
            .await
            .unwrap()
            .credential_storage_ready
    );
    assert_eq!(
        degraded
            .plugins
            .settings(&actor)
            .await
            .unwrap()
            .installations
            .len(),
        1
    );
    assert!(
        degraded
            .plugins
            .refresh(
                &actor,
                &installed.id,
                RefreshPlugin {
                    revision: installed.revision + 1,
                    accept_manifest_digest: None,
                    credential: PluginCredential::Keep
                }
            )
            .await
            .is_err()
    );
    assert_eq!(server.hits(), 2);
    std::fs::write(dir.path().join("hito.plugin-key"), &key).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.path().join("hito.plugin-key"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    let restored = hito_core::open(config()).await.unwrap();
    assert!(
        restored
            .plugins
            .settings(&actor)
            .await
            .unwrap()
            .credential_storage_ready
    );
    restored
        .plugins
        .refresh(
            &actor,
            &installed.id,
            RefreshPlugin {
                revision: installed.revision + 1,
                accept_manifest_digest: None,
                credential: PluginCredential::Keep,
            },
        )
        .await
        .unwrap();
    let cleared = degraded
        .plugins
        .clear_credential(&actor, &installed.id, installed.revision + 2)
        .await
        .unwrap();
    assert!(!cleared.credential_set);
    assert_eq!(
        server.hits(),
        3,
        "clearing a saved credential does not contact the provider"
    );
}
