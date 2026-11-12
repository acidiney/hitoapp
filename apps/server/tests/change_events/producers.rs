use super::{
    PASSWORD, admin, app, call, guest, login, notice, playlist_fixture, playlist_notice,
    provider_fixture, quiet, stream, support,
};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

fn sources_notice(recording: &str) -> Value {
    json!({"kind":"sources","recording_id":recording})
}

async fn provider(app: &Router, token: &str) -> (provider_fixture::ProviderServer, Value) {
    let server = provider_fixture::ProviderServer::start().await;
    server
        .reply(
            "v1/recordings/detail",
            json!({
                "id": "owned", "title": "Owned title", "artist": "Owned artist", "duration_ms": 90_000
            }),
        )
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": "owned-audio", "label": "Owned audio", "ready": true}]}),
        )
        .await;
    let (status, service) = call(
        app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Change notice provider", "base_url": server.url}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(
        app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null}),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    (server, installation)
}

#[tokio::test]
async fn provider_source_changes_reach_only_accounts_that_can_read_the_recording() {
    let (_dir, app) = app().await;
    let owner = admin(&app).await;
    let reader = guest(&app, &owner).await;
    let mut owner_changes = stream(&app, &owner).await;
    let mut other_changes = stream(&app, &reader).await;
    let (server, installation) = provider(&app, &owner).await;

    let (status, adopted) = call(
        &app,
        "POST",
        &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()),
        json!({"installation_revision": installation["revision"], "provider_recording_id": "owned"}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let recording = adopted["id"].as_str().unwrap().to_owned();
    assert_eq!(notice(&mut owner_changes).await, sources_notice(&recording));

    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{recording}/sources/resolve"),
        json!({"request_id": Uuid::new_v4()}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notice(&mut owner_changes).await, sources_notice(&recording));

    let refresh = format!("/api/v1/recordings/{recording}/sources/refresh");
    assert_eq!(
        call(&app, "POST", &refresh, Value::Null, Some(&owner)).await.0,
        StatusCode::OK
    );
    assert_eq!(notice(&mut owner_changes).await, sources_notice(&recording));

    server.offline().await;
    assert_ne!(
        call(&app, "POST", &refresh, Value::Null, Some(&owner)).await.0,
        StatusCode::OK
    );
    server.online().await;
    let candidate = json!({
        "installation_id": installation["id"], "installation_revision": installation["revision"],
        "provider_recording_id": "owned"
    });
    assert_ne!(
        call(
            &app,
            "POST",
            &format!("/api/v1/recordings/{recording}/sources/attach"),
            json!({"candidate": candidate, "review_stamp": "provider-source-v1:stale"}),
            Some(&owner),
        )
        .await
        .0,
        StatusCode::OK
    );
    quiet(&mut owner_changes).await;
    quiet(&mut other_changes).await;
}

#[tokio::test]
async fn attaching_a_source_to_a_shared_library_recording_notifies_every_member_and_no_outsider() {
    let (_dir, app, owner, recording) = support::indexed_app().await;
    let (status, member) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "member", "password": PASSWORD}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "outsider", "password": PASSWORD}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&owner)).await;
    let library = libraries[0]["id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!(
                "/api/v1/libraries/{library}/members/{}",
                member["id"].as_str().unwrap()
            ),
            Value::Null,
            Some(&owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let member = login(&app, "member").await;
    let outsider = login(&app, "outsider").await;
    let mut owner_changes = stream(&app, &owner).await;
    let mut member_changes = stream(&app, &member).await;
    let mut outsider_changes = stream(&app, &outsider).await;
    let (_server, installation) = provider(&app, &owner).await;
    let candidate = json!({
        "installation_id": installation["id"], "installation_revision": installation["revision"],
        "provider_recording_id": "owned"
    });
    let (status, review) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{recording}/sources/review"),
        candidate.clone(),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    let (status, attached) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{recording}/sources/attach"),
        json!({"candidate": candidate, "review_stamp": review["review_stamp"]}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{attached}");
    assert_eq!(notice(&mut owner_changes).await, sources_notice(&recording));
    assert_eq!(notice(&mut member_changes).await, sources_notice(&recording));
    quiet(&mut outsider_changes).await;
    quiet(&mut owner_changes).await;
    quiet(&mut member_changes).await;
}

async fn imported(
    app: &Router,
    token: &str,
    provider: &playlist_fixture::PlaylistProvider,
) -> (Value, String) {
    let body = json!({
        "request_id": Uuid::new_v4(),
        "installation_revision": provider.revision,
        "playlist_reference": "https://provider.invalid/playlists/change-notices"
    });
    let (status, receipt) =
        call(app, "POST", &provider.import_path, body.clone(), Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    (body, receipt["playlist_id"].as_str().unwrap().to_owned())
}

#[tokio::test]
async fn importing_a_provider_playlist_notifies_its_owner_once_after_commit() {
    let (_dir, app) = app().await;
    let owner = admin(&app).await;
    let other = guest(&app, &owner).await;
    let mut owner_changes = stream(&app, &owner).await;
    let mut other_changes = stream(&app, &other).await;
    let provider = playlist_fixture::connect(&app, &owner).await;

    let (body, playlist) = imported(&app, &owner, &provider).await;
    assert_eq!(notice(&mut owner_changes).await, playlist_notice(&playlist));
    assert_eq!(notice(&mut owner_changes).await, json!({"kind":"playlists"}));

    let (status, replay) = call(&app, "POST", &provider.import_path, body, Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["playlist_id"], playlist.as_str());
    assert_ne!(
        call(
            &app,
            "POST",
            &provider.import_path,
            json!({
                "request_id": Uuid::new_v4(),
                "installation_revision": provider.revision + 7,
                "playlist_reference": "https://provider.invalid/playlists/stale-connection"
            }),
            Some(&owner),
        )
        .await
        .0,
        StatusCode::OK
    );
    quiet(&mut owner_changes).await;
    quiet(&mut other_changes).await;
}

#[tokio::test]
async fn refreshing_imported_playlist_metadata_notifies_its_owner_after_commit() {
    let (_dir, app) = app().await;
    let owner = admin(&app).await;
    let provider = playlist_fixture::connect(&app, &owner).await;
    let (_, playlist) = imported(&app, &owner, &provider).await;
    let mut changes = stream(&app, &owner).await;
    let refresh = format!("/api/v1/playlists/{playlist}/metadata/refresh");

    assert_ne!(
        call(
            &app,
            "POST",
            &refresh,
            json!({"request_id": Uuid::new_v4(), "installation_revision": provider.revision + 7}),
            Some(&owner),
        )
        .await
        .0,
        StatusCode::OK
    );
    quiet(&mut changes).await;

    assert_eq!(
        call(
            &app,
            "POST",
            &refresh,
            json!({"request_id": Uuid::new_v4(), "installation_revision": provider.revision}),
            Some(&owner),
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(notice(&mut changes).await, playlist_notice(&playlist));
    assert_eq!(notice(&mut changes).await, json!({"kind":"playlists"}));
    assert_eq!(notice(&mut changes).await, json!({"kind":"recordings"}));
    quiet(&mut changes).await;
}

#[tokio::test]
async fn following_and_refreshing_a_playlist_notify_its_owner_only_when_entries_publish() {
    let dir = tempfile::tempdir().unwrap();
    let services = support::services(dir.path(), &dir.path().join("music")).await;
    let app = hito_server::router(services.clone());
    let owner = admin(&app).await;
    let provider = playlist_fixture::connect(&app, &owner).await;
    let mut changes = stream(&app, &owner).await;

    let (status, followed) = call(
        &app,
        "POST",
        &format!("/api/v1/plugins/{}/playlists/follow", provider.installation_id),
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": provider.revision,
            "playlist_reference": "https://provider.invalid/playlists/followed-notices",
            "interval": "hourly"
        }),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let playlist = followed["playlist_id"].as_str().unwrap().to_owned();
    assert_eq!(notice(&mut changes).await, playlist_notice(&playlist));
    assert_eq!(notice(&mut changes).await, json!({"kind":"playlists"}));

    let refresh = format!("/api/v1/playlists/{playlist}/follow/refresh");
    provider.server.offline().await;
    assert_eq!(
        call(&app, "POST", &refresh, json!({"request_id": Uuid::new_v4()}), Some(&owner))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    assert!(
        services
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    quiet(&mut changes).await;

    provider.server.online().await;
    retry_now(&dir).await;
    assert!(
        services
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    quiet(&mut changes).await;

    provider
        .server
        .reply(
            "v1/playlists/detail",
            json!({
                "id": "playlist/retained", "name": "Provider selection", "snapshot": "snapshot-two",
                "total_count": 1, "attribution": {"label": "Curated by the fixture owner"}
            }),
        )
        .await;
    provider
        .server
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": "playlist/retained", "snapshot": "snapshot-two",
                "items": [{"occurrence_id": "only", "item": {"kind": "recording", "recording": {
                    "id": "recording/known", "title": "Known recording", "artist": "Fixture artist",
                    "duration_ms": 123_000
                }}}],
                "next_cursor": null
            }),
        )
        .await;
    assert_eq!(
        call(&app, "POST", &refresh, json!({"request_id": Uuid::new_v4()}), Some(&owner))
            .await
            .0,
        StatusCode::ACCEPTED
    );
    assert!(
        services
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(notice(&mut changes).await, playlist_notice(&playlist));
    assert_eq!(notice(&mut changes).await, json!({"kind":"playlists"}));
    quiet(&mut changes).await;
}

async fn retry_now(dir: &tempfile::TempDir) {
    super::database(
        dir,
        "UPDATE playlist_follow_jobs SET next_attempt_at = unixepoch() - ? WHERE status = 'queued'",
        1,
    )
    .await;
}

#[tokio::test]
async fn arriving_playlist_artwork_notifies_its_owner_after_the_worker_commits() {
    use hito_core::test_support::collection_artwork::ArtworkServer;
    let dir = tempfile::tempdir().unwrap();
    let services = support::services(dir.path(), &dir.path().join("music")).await;
    let app = hito_server::router(services.clone());
    let owner = admin(&app).await;
    let actor = services.identity.authenticate(&owner).await.unwrap();
    let provider = ArtworkServer::start().await;
    let installed = provider.connect(&services, &actor).await;
    let playlist = services
        .collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            hito_core::ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlists/artwork-notices".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id
        .to_string();
    let other = guest(&app, &owner).await;
    let mut owner_changes = stream(&app, &owner).await;
    let mut other_changes = stream(&app, &other).await;

    let fetch = format!("/api/v1/playlists/{playlist}/artwork/fetch");
    let (status, queued) = call(&app, "POST", &fetch, Value::Null, Some(&owner)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(queued["status"], "queued");
    quiet(&mut owner_changes).await;

    assert!(
        services
            .catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    );
    assert_eq!(
        notice(&mut owner_changes).await,
        json!({"kind":"artwork","entity":"playlist","id":playlist})
    );
    assert_eq!(notice(&mut owner_changes).await, playlist_notice(&playlist));
    assert_eq!(notice(&mut owner_changes).await, json!({"kind":"playlists"}));

    let (status, retained) = call(&app, "POST", &fetch, Value::Null, Some(&owner)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(retained["status"], "ready");
    quiet(&mut owner_changes).await;
    quiet(&mut other_changes).await;
}
