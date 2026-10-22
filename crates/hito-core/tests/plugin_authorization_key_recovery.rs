use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    CoreConfig, Credentials, Error, HostDeviceId, InstallPlugin, OfferPluginService,
    PluginAuthorizationRedirect, PluginAuthorizationState,
};
use serde_json::json;
use sqlx::{Connection, sqlite::SqliteConnectOptions};

#[tokio::test]
async fn live_consent_secrets_protect_a_missing_key_until_the_attempt_expires() {
    let provider = fixture::ProviderServer::start().await;
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.authorization-key-fixture",
                "name": "Authorization key fixture",
                "protocol_version": 1,
                "capabilities": ["playlists"],
                "authorization": {
                    "kind": "oauth2_pkce",
                    "client_id": "owned-test-client",
                    "authorization_endpoint": "https://accounts.spotify.com/authorize",
                    "scopes": ["playlist-read-private"]
                }
            }),
        )
        .await;
    let (directory, hito, actor, _library, music) =
        support::library("authorization-key-host").await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Authorization key fixture".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let redirect =
        PluginAuthorizationRedirect::new("https://hito.test/api/v1/plugin-authorization/callback")
            .unwrap();
    let pending = hito
        .plugins
        .begin_authorization(&actor, &installation.id, installation.revision, &redirect)
        .await
        .unwrap();
    let status = hito
        .plugins
        .authorization_status(&actor, &installation.id, &pending.id)
        .await
        .unwrap();
    assert!(matches!(status.status, PluginAuthorizationState::Pending));
    let database = directory.path().join("hito.db");
    let key_path = database.with_extension("plugin-key");
    std::fs::remove_file(&key_path).unwrap();
    drop(hito);
    let config = || {
        CoreConfig::new(
            database.clone(),
            music.clone(),
            HostDeviceId::new("authorization-key-host").unwrap(),
        )
    };

    let protected = hito_core::open(config()).await.unwrap();
    assert!(protected.healthy().await);
    assert!(
        !protected
            .plugins
            .settings(&actor)
            .await
            .unwrap()
            .credential_storage_ready
    );
    assert!(matches!(
        protected
            .plugins
            .begin_authorization(&actor, &installation.id, installation.revision, &redirect)
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(
        !key_path.exists(),
        "live consent secrets must not permit key replacement"
    );
    drop(protected);

    expire_persisted_fixture(&database, &pending.id).await;
    let recovered = hito_core::open(config()).await.unwrap();
    let session = recovered
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let fresh_actor = recovered
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(
        recovered
            .plugins
            .settings(&fresh_actor)
            .await
            .unwrap()
            .credential_storage_ready,
        "expired-only consent secrets must not prevent fresh key creation"
    );
    assert_eq!(std::fs::metadata(&key_path).unwrap().len(), 32);
    assert!(matches!(
        recovered
            .plugins
            .authorization_status(&actor, &installation.id, &pending.id)
            .await,
        Err(Error::Unauthorized)
    ));
    let restarted = recovered
        .plugins
        .begin_authorization(
            &fresh_actor,
            &installation.id,
            installation.revision,
            &redirect,
        )
        .await
        .unwrap();
    assert_ne!(restarted.id, pending.id);
    let status = recovered
        .plugins
        .authorization_status(&fresh_actor, &installation.id, &restarted.id)
        .await
        .unwrap();
    assert!(matches!(status.status, PluginAuthorizationState::Pending));
    assert_eq!(status.id, restarted.id);
    assert_eq!(status.expires_at, restarted.expires_at);
}

async fn expire_persisted_fixture(database: &std::path::Path, attempt: &str) {
    let mut connection = sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(database)
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut transaction = connection.begin().await.unwrap();
    sqlx::query("UPDATE plugin_authorization_attempts SET created_at = unixepoch() - 601, expires_at = unixepoch() - 1 WHERE id = ?")
        .bind(attempt).execute(&mut *transaction).await.unwrap();
    sqlx::query("UPDATE sessions SET expires_at = unixepoch() - 1 WHERE token_hash = (SELECT session_hash FROM plugin_authorization_attempts WHERE id = ?)")
        .bind(attempt).execute(&mut *transaction).await.unwrap();
    transaction.commit().await.unwrap();
    connection.close().await.unwrap();
}
