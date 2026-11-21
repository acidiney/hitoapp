use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, Credentials, Error, FollowProviderPlaylist, InstallPlugin, Operation,
    PlaylistFollowInterval, PluginInstallation, RefreshFollowedPlaylist, Services, SyncBatch,
};
use serde_json::{Value, json};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

struct Followed {
    server: fixture::ProviderServer,
    hito: Services,
    actor: Actor,
    installation: PluginInstallation,
    playlist: String,
    directory: tempfile::TempDir,
}

async fn followed(host: &str) -> Followed {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library(host).await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    snapshot(&server, "first").await;
    let receipt = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-reconnect".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    Followed {
        server,
        hito,
        actor,
        installation,
        playlist: receipt.playlist_id.to_string(),
        directory,
    }
}

async fn snapshot(server: &fixture::ProviderServer, version: &str) {
    fixture::playlist(
        server,
        "playlist/reconnect",
        version,
        vec![
            json!({"occurrence_id": version, "item": {"kind": "recording", "recording": {
                "id": format!("recording/{version}"), "title": format!("Reconnect {version}"),
                "artist": "Owned reconnect artist", "duration_ms": 100000
            }}}),
        ],
    )
    .await;
}

async fn remove(scenario: &Followed, installation: &PluginInstallation) {
    scenario
        .hito
        .plugins
        .remove(&scenario.actor, &installation.id, installation.revision)
        .await
        .unwrap();
}

async fn connect_as(
    scenario: &Followed,
    actor: &Actor,
    capabilities: &[&str],
    token: Option<&str>,
) -> PluginInstallation {
    scenario
        .server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": "Playlist fixture",
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let service = scenario
        .hito
        .plugins
        .settings(actor)
        .await
        .unwrap()
        .services
        .into_iter()
        .find(|service| service.id == scenario.installation.service_id)
        .unwrap();
    scenario
        .hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: token.map(str::to_owned),
                accept_manifest_digest: None,
            },
        )
        .await
        .unwrap()
}

async fn reconnect(scenario: &Followed, capabilities: &[&str]) -> PluginInstallation {
    connect_as(scenario, &scenario.actor, capabilities, None).await
}

async fn status(scenario: &Followed) -> Value {
    serde_json::to_value(
        scenario
            .hito
            .collection
            .playlist_follow(&scenario.actor, &scenario.playlist)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn refreshes(scenario: &Followed) -> bool {
    scenario
        .hito
        .collection
        .run_next_follow_refresh_until(std::future::pending())
        .await
        .unwrap()
}

async fn origin_installation(scenario: &Followed) -> String {
    scenario
        .hito
        .collection
        .playlist(&scenario.actor, &scenario.playlist)
        .await
        .unwrap()
        .origin
        .unwrap()
        .installation_id
}

async fn resume(scenario: &Followed, paused: &Value) {
    support::synchronize(
        &scenario.hito,
        &scenario.actor,
        vec![Action::PlaylistFollowResume {
            playlist_id: Uuid::parse_str(&scenario.playlist).unwrap(),
            expected_generation: paused["generation"].as_i64().unwrap(),
            installation_revision: paused["review"]["installation_revision"].as_i64().unwrap(),
            service_revision: paused["review"]["service_revision"].as_i64().unwrap(),
        }],
    )
    .await;
}

async fn database(scenario: &Followed) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(scenario.directory.path().join("hito.db")),
    )
    .await
    .unwrap()
}

async fn strand(scenario: &Followed) {
    let mut connection = database(scenario).await;
    sqlx::query("UPDATE playlist_follows SET installation_id = ?,installation_revision = ? WHERE playlist_id = ?")
        .bind(&scenario.installation.id)
        .bind(scenario.installation.revision)
        .bind(&scenario.playlist)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
async fn a_reconnect_carries_the_follow_over_and_it_refreshes_on_the_new_connection() {
    let scenario = followed("follow-reconnect-carry").await;
    let before = status(&scenario).await;
    remove(&scenario, &scenario.installation).await;
    assert_eq!(status(&scenario).await["status"], "paused");
    let replacement = reconnect(&scenario, &["playlists"]).await;
    let after = status(&scenario).await;
    assert_ne!(after["status"], "paused");
    assert!(after["review"].is_null());
    assert!(after["error"].is_null());
    assert!(after["generation"].as_i64() > before["generation"].as_i64());
    snapshot(&scenario.server, "second").await;
    assert!(refreshes(&scenario).await);
    assert_eq!(origin_installation(&scenario).await, replacement.id);
    let detail = scenario
        .hito
        .collection
        .playlist(&scenario.actor, &scenario.playlist)
        .await
        .unwrap();
    assert_eq!(detail.entries.len(), 1);
    assert_eq!(status(&scenario).await["status"], "ready");
    scenario
        .hito
        .collection
        .enqueue_follow_refresh(
            &scenario.actor,
            &scenario.playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_reconnect_with_changed_abilities_asks_for_review_and_resumes_on_the_new_connection() {
    let scenario = followed("follow-reconnect-review").await;
    remove(&scenario, &scenario.installation).await;
    let replacement = reconnect(&scenario, &["playlists", "search"]).await;
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    let review = &paused["review"];
    assert_eq!(review["resumable"], true);
    assert_eq!(review["installation_revision"], replacement.revision);
    assert_eq!(
        review["changes"],
        json!([{"kind": "capabilities", "before": ["playlists"], "after": ["search", "playlists"]}])
    );
    assert!(
        review
            .get("reconnect_service_id")
            .is_none_or(Value::is_null)
    );
    assert!(!refreshes(&scenario).await);
    resume(&scenario, &paused).await;
    let resumed = status(&scenario).await;
    assert_eq!(resumed["status"], "ready");
    assert!(resumed["review"].is_null());
    snapshot(&scenario.server, "second").await;
    assert!(refreshes(&scenario).await);
    assert_eq!(origin_installation(&scenario).await, replacement.id);
}

#[tokio::test]
async fn a_reconnect_with_a_new_token_asks_for_review() {
    let scenario = followed("follow-reconnect-token").await;
    remove(&scenario, &scenario.installation).await;
    connect_as(
        &scenario,
        &scenario.actor,
        &["playlists"],
        Some("owned-reconnect-token"),
    )
    .await;
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["review"]["resumable"], true);
    assert_eq!(
        paused["review"]["changes"],
        json!([{"kind": "credential", "before": ["No token"], "after": ["Saved token"]}])
    );
    assert!(!paused.to_string().contains("owned-reconnect-token"));
    resume(&scenario, &paused).await;
    assert_eq!(status(&scenario).await["status"], "ready");
}

#[tokio::test]
async fn a_removed_connection_without_a_new_one_stays_removed_and_names_the_service_to_connect() {
    let scenario = followed("follow-reconnect-removed").await;
    remove(&scenario, &scenario.installation).await;
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    let review = &paused["review"];
    assert_eq!(review["resumable"], false);
    assert_eq!(review["connection_name"], "Playlist fixture");
    assert_eq!(review["changes"][0]["kind"], "removed");
    assert_eq!(
        review["reconnect_service_id"],
        scenario.installation.service_id
    );
    assert!(!refreshes(&scenario).await);
    assert!(matches!(
        scenario
            .hito
            .collection
            .enqueue_follow_refresh(
                &scenario.actor,
                &scenario.playlist,
                RefreshFollowedPlaylist {
                    request_id: Uuid::new_v4(),
                },
            )
            .await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn another_accounts_connection_of_the_same_plugin_is_never_used() {
    let scenario = followed("follow-reconnect-privacy").await;
    remove(&scenario, &scenario.installation).await;
    scenario
        .hito
        .identity
        .create_user(
            &scenario.actor,
            Credentials {
                username: "other".into(),
                password: "other reconnect password".into(),
            },
        )
        .await
        .unwrap();
    let session = scenario
        .hito
        .identity
        .login(Credentials {
            username: "other".into(),
            password: "other reconnect password".into(),
        })
        .await
        .unwrap();
    let other = scenario
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    connect_as(&scenario, &other, &["playlists"], None).await;
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["review"]["changes"][0]["kind"], "removed");
    assert!(!refreshes(&scenario).await);
    let mut connection = database(&scenario).await;
    let bound: String =
        sqlx::query_scalar("SELECT installation_id FROM playlist_follows WHERE playlist_id = ?")
            .bind(&scenario.playlist)
            .fetch_one(&mut connection)
            .await
            .unwrap();
    connection.close().await.unwrap();
    assert_eq!(bound, scenario.installation.id);
}

#[tokio::test]
async fn a_follow_stranded_before_the_fix_recovers_when_it_is_reviewed() {
    let scenario = followed("follow-reconnect-stranded").await;
    remove(&scenario, &scenario.installation).await;
    let replacement = reconnect(&scenario, &["playlists"]).await;
    strand(&scenario).await;
    let recovered = status(&scenario).await;
    assert_ne!(recovered["status"], "paused");
    assert!(recovered["review"].is_null());
    snapshot(&scenario.server, "second").await;
    assert!(refreshes(&scenario).await);
    assert_eq!(origin_installation(&scenario).await, replacement.id);
}

#[tokio::test]
async fn the_worker_recovers_a_stranded_follow_without_a_review() {
    let scenario = followed("follow-reconnect-stranded-worker").await;
    remove(&scenario, &scenario.installation).await;
    let replacement = reconnect(&scenario, &["playlists"]).await;
    strand(&scenario).await;
    snapshot(&scenario.server, "second").await;
    assert!(refreshes(&scenario).await);
    assert_eq!(origin_installation(&scenario).await, replacement.id);
}

#[tokio::test]
async fn a_stranded_follow_with_changed_abilities_shows_resume_and_resumes() {
    let scenario = followed("follow-reconnect-stranded-review").await;
    remove(&scenario, &scenario.installation).await;
    let replacement = reconnect(&scenario, &["playlists", "lyrics"]).await;
    strand(&scenario).await;
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["review"]["resumable"], true);
    assert_eq!(paused["review"]["changes"][0]["kind"], "capabilities");
    assert_eq!(
        paused["review"]["installation_revision"],
        replacement.revision
    );
    resume(&scenario, &paused).await;
    assert_eq!(status(&scenario).await["status"], "ready");
    snapshot(&scenario.server, "second").await;
    assert!(refreshes(&scenario).await);
    assert_eq!(origin_installation(&scenario).await, replacement.id);
}

#[tokio::test]
async fn a_review_of_a_replaced_connection_cannot_approve_the_next_one() {
    let scenario = followed("follow-reconnect-stale").await;
    remove(&scenario, &scenario.installation).await;
    let first = reconnect(&scenario, &["playlists", "search"]).await;
    let stale = status(&scenario).await;
    assert_eq!(stale["review"]["resumable"], true);
    remove(&scenario, &first).await;
    let second = reconnect(&scenario, &["playlists", "search", "lyrics"]).await;
    assert_eq!(first.revision, second.revision);
    let outcome = scenario
        .hito
        .collection
        .apply(
            &scenario.actor,
            SyncBatch {
                device_id: "reconnect-test".into(),
                operations: vec![Operation {
                    id: Uuid::new_v4(),
                    action: Action::PlaylistFollowResume {
                        playlist_id: Uuid::parse_str(&scenario.playlist).unwrap(),
                        expected_generation: stale["generation"].as_i64().unwrap(),
                        installation_revision: stale["review"]["installation_revision"]
                            .as_i64()
                            .unwrap(),
                        service_revision: stale["review"]["service_revision"].as_i64().unwrap(),
                    },
                }],
            },
        )
        .await;
    assert!(matches!(outcome, Err(Error::Conflict(_))));
    let current = status(&scenario).await;
    assert_eq!(current["status"], "paused");
    assert_eq!(
        current["review"]["changes"][0]["after"],
        json!(["search", "playlists", "lyrics"])
    );
    resume(&scenario, &current).await;
    assert_eq!(status(&scenario).await["status"], "ready");
}
