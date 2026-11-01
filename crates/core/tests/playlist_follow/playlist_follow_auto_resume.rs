use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Actor, FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus, PluginCredential,
    PluginInstallation, PluginRefresh, RefreshPlugin, Services,
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
    _directory: tempfile::TempDir,
}

async fn followed(host: &str, capabilities: &[&str]) -> Followed {
    let server = fixture::ProviderServer::start().await;
    let (directory, hito, actor, _shared, _music) = support::library(host).await;
    let installation = fixture::connect(&hito, &actor, &server, capabilities).await;
    fixture::playlist(
        &server,
        "playlist/auto-resume",
        "first",
        vec![json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
            "id": "recording/one", "title": "Auto resume one", "artist": "Owned artist", "duration_ms": 100000
        }}})],
    )
    .await;
    let receipt = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-auto-resume".into(),
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
        _directory: directory,
    }
}

async fn reconnect(
    scenario: &Followed,
    installation: &PluginInstallation,
    name: &str,
    capabilities: &[&str],
    credential: Option<&str>,
) -> PluginInstallation {
    scenario
        .server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": name,
                "protocol_version": 1, "capabilities": capabilities
            }),
        )
        .await;
    let request = |accept: Option<String>| RefreshPlugin {
        revision: installation.revision,
        accept_manifest_digest: accept,
        credential: credential
            .map(|token| PluginCredential::Replace {
                bearer_token: token.into(),
            })
            .unwrap_or_default(),
    };
    let first = scenario
        .hito
        .plugins
        .refresh(&scenario.actor, &installation.id, request(None))
        .await
        .unwrap();
    match first {
        PluginRefresh::Updated { installation } => installation,
        PluginRefresh::ReviewRequired { digest, .. } => {
            match scenario
                .hito
                .plugins
                .refresh(&scenario.actor, &installation.id, request(Some(digest)))
                .await
                .unwrap()
            {
                PluginRefresh::Updated { installation } => installation,
                _ => panic!("The reviewed manifest must be accepted"),
            }
        }
    }
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

#[tokio::test]
async fn a_revision_bump_with_the_same_capabilities_keeps_updating_without_review() {
    let scenario = followed("follow-auto-same", &["playlists", "artwork"]).await;
    let before = status(&scenario).await;
    let renamed = reconnect(
        &scenario,
        &scenario.installation,
        "Rebuilt fixture",
        &["playlists", "artwork"],
        None,
    )
    .await;
    assert!(renamed.revision > scenario.installation.revision);
    let after = status(&scenario).await;
    assert_ne!(after["status"], "paused");
    assert_eq!(
        after["generation"],
        before["generation"].as_i64().unwrap() + 1
    );
    assert!(after["error"].is_null());
    assert!(after.get("review").is_none_or(Value::is_null));
    fixture::playlist(&scenario.server, "playlist/auto-resume", "second", vec![]).await;
    assert!(
        scenario
            .hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let detail = scenario
        .hito
        .collection
        .playlist(&scenario.actor, &scenario.playlist)
        .await
        .unwrap();
    assert_eq!(
        detail.origin.unwrap().installation_revision,
        renamed.revision
    );
    assert_eq!(status(&scenario).await["status"], "ready");
}

#[tokio::test]
async fn the_worker_resumes_an_unobserved_compatible_revision_change_by_itself() {
    let scenario = followed("follow-auto-worker", &["playlists", "search"]).await;
    let narrowed = reconnect(
        &scenario,
        &scenario.installation,
        "Narrower fixture",
        &["playlists"],
        None,
    )
    .await;
    fixture::playlist(&scenario.server, "playlist/auto-resume", "second", vec![]).await;
    assert!(
        scenario
            .hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let detail = scenario
        .hito
        .collection
        .playlist(&scenario.actor, &scenario.playlist)
        .await
        .unwrap();
    assert_eq!(
        detail.origin.unwrap().installation_revision,
        narrowed.revision
    );
    assert_eq!(detail.entries.len(), 0);
}

#[tokio::test]
async fn gaining_an_ability_or_changing_the_credential_still_pauses_the_follow() {
    let scenario = followed("follow-auto-pause", &["playlists"]).await;
    let widened = reconnect(
        &scenario,
        &scenario.installation,
        "Wider fixture",
        &["playlists", "search"],
        None,
    )
    .await;
    assert_eq!(status(&scenario).await["status"], "paused");
    assert!(
        !scenario
            .hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let _ = widened;

    let scenario = followed("follow-auto-token", &["playlists"]).await;
    reconnect(
        &scenario,
        &scenario.installation,
        "Playlist fixture",
        &["playlists"],
        Some("a-new-private-token"),
    )
    .await;
    assert_eq!(status(&scenario).await["status"], "paused");
}

#[tokio::test]
async fn removing_the_playlists_ability_pauses_the_follow() {
    let scenario = followed("follow-auto-removed", &["playlists", "search"]).await;
    reconnect(
        &scenario,
        &scenario.installation,
        "Search only fixture",
        &["search"],
        None,
    )
    .await;
    assert_eq!(
        status(&scenario).await["status"],
        json!(PlaylistFollowStatus::Paused)
    );
}

#[tokio::test]
async fn the_review_names_only_what_changed_and_the_revisions_to_approve() {
    let scenario = followed("follow-auto-review", &["playlists", "artwork"]).await;
    let widened = reconnect(
        &scenario,
        &scenario.installation,
        "Wider fixture",
        &["playlists", "artwork", "search"],
        None,
    )
    .await;
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    let review = &paused["review"];
    assert_eq!(review["connection_name"], "Wider fixture");
    assert_eq!(review["resumable"], true);
    assert_eq!(review["installation_revision"], widened.revision);
    assert_eq!(
        review["changes"],
        json!([{"kind": "capabilities",
            "before": ["playlists", "artwork"],
            "after": ["search", "playlists", "artwork"]}])
    );

    let tokened = followed("follow-auto-review-token", &["playlists"]).await;
    reconnect(
        &tokened,
        &tokened.installation,
        "Playlist fixture",
        &["playlists"],
        Some("another-private-token"),
    )
    .await;
    let review = &status(&tokened).await["review"];
    assert_eq!(
        review["changes"],
        json!([{"kind": "credential", "before": ["No token"], "after": ["Saved token"]}])
    );
    assert!(!review.to_string().contains("another-private-token"));
}

#[tokio::test]
async fn a_removed_playlists_ability_cannot_be_resumed_and_a_ready_follow_has_no_review() {
    let scenario = followed("follow-auto-review-removed", &["playlists", "search"]).await;
    assert!(status(&scenario).await["review"].is_null());
    reconnect(
        &scenario,
        &scenario.installation,
        "Search only fixture",
        &["search"],
        None,
    )
    .await;
    let review = &status(&scenario).await["review"];
    assert_eq!(review["resumable"], false);
    assert_eq!(review["changes"][0]["kind"], "capabilities");
    assert_eq!(review["changes"][0]["after"], json!(["search"]));
}

#[tokio::test]
async fn a_follow_without_a_recorded_approval_stays_paused_and_asks_for_approval() {
    let scenario = followed("follow-auto-legacy", &["playlists"]).await;
    reconnect(
        &scenario,
        &scenario.installation,
        "Wider fixture",
        &["playlists", "search"],
        None,
    )
    .await;
    assert_eq!(status(&scenario).await["status"], "paused");
    clear_baseline(&scenario).await;
    assert!(
        !scenario
            .hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let paused = status(&scenario).await;
    assert_eq!(paused["status"], "paused");
    assert_eq!(paused["review"]["resumable"], true);
    assert_eq!(
        paused["review"]["changes"],
        json!([{"kind": "capabilities", "before": [], "after": ["search", "playlists"]}])
    );
}

#[tokio::test]
async fn a_malformed_recorded_approval_is_not_compatible_and_does_not_fail_the_poll() {
    let scenario = followed("follow-auto-malformed", &["playlists"]).await;
    reconnect(
        &scenario,
        &scenario.installation,
        "Rebuilt fixture",
        &["playlists"],
        None,
    )
    .await;
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(scenario._directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE playlist_follows SET approved_capabilities = '{broken',generation = generation",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    scenario
        .hito
        .collection
        .run_next_follow_refresh_until(std::future::pending())
        .await
        .unwrap();
    assert_eq!(status(&scenario).await["status"], "paused");
}

async fn clear_baseline(scenario: &Followed) {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new().filename(scenario._directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE playlist_follows SET approved_capabilities = NULL,approved_credential = NULL",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
}
