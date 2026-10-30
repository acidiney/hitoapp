use hito_core::test_support::plugin_service as fixture;
use hito_core::test_support::support;
use hito_core::{CoreConfig, HostDeviceId, InstallPlugin, OfferPluginService};

#[tokio::test]
async fn invalid_keys_do_not_replace_encrypted_credentials_or_stop_local_services() {
    let server = fixture::PluginServer::start().await;
    let (dir, hito, actor, _library, music) = support::library("plugin-key-host").await;
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
                service_revision: 1,
                bearer_token: Some("key-recovery-token".into()),
            },
        )
        .await
        .unwrap();
    let key_path = dir.path().join("hito.plugin-key");
    let original = std::fs::read(&key_path).unwrap();
    let config = || {
        CoreConfig::new(
            dir.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("plugin-key-host").unwrap(),
        )
    };
    for bad in [vec![0u8; 32], vec![1u8; 31], vec![2u8; 33]] {
        std::fs::write(&key_path, &bad).unwrap();
        let reopened = hito_core::open(config()).await.unwrap();
        assert!(reopened.healthy().await);
        let settings = reopened.plugins.settings(&actor).await.unwrap();
        assert!(!settings.credential_storage_ready);
        assert!(settings.installations[0].credential_set);
        assert_eq!(
            std::fs::read(&key_path).unwrap(),
            bad,
            "an invalid key is never silently replaced"
        );
    }
    std::fs::write(&key_path, &original).unwrap();
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
            hito_core::RefreshPlugin {
                revision: 1,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap();
}
