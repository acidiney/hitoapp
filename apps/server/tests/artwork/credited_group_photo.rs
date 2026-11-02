use hito_core::test_support::collection_artwork::{ArtistKind, ArtworkServer};
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use hito_core::ImportProviderPlaylist;
use sqlx::Connection;
use support::{admin, call};
use uuid::Uuid;

#[tokio::test]
async fn a_group_lists_and_opens_with_the_account_artist_whose_photo_the_plugin_serves() {
    let directory = tempfile::tempdir().unwrap();
    let hito = support::services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let actor = hito.identity.authenticate(&token).await.unwrap();
    let provider = ArtworkServer::start_with_credited_artist(ArtistKind::Served).await;
    let installed = provider.connect(&hito, &actor).await;
    hito.collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
            },
        )
        .await
        .unwrap();

    let (_, credits) = call(
        &app,
        "GET",
        "/api/v1/artists",
        serde_json::Value::Null,
        Some(&token),
    )
    .await;
    let artist = credits["items"][0]["id"].as_str().unwrap().to_owned();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(directory.path().join("hito.db"))
        .foreign_keys(false);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    for statement in [
        "INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,plugin_installation_id,provider_artist_id) VALUES('group',(SELECT id FROM users),'http_plugin',(SELECT id FROM plugin_installations),'plugin-artist')",
        "INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence) SELECT 'association',(SELECT id FROM users),library_id,id,(SELECT id FROM account_artists),metadata_observation_id,'recording_artist','http_plugin',(SELECT id FROM plugin_installations),'plugin-artist','user_confirmed' FROM recordings",
        "INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) SELECT (SELECT id FROM users),id,1,'association','group',(SELECT id FROM account_artists),metadata_observation_id,1 FROM recordings",
    ] {
        sqlx::query(statement)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    connection.close().await.unwrap();

    let (status, list) = call(
        &app,
        "GET",
        "/api/v1/artist-identity-groups",
        serde_json::Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["items"][0]["credited_photo_artist_id"],
        artist.as_str()
    );
    let (status, detail) = call(
        &app,
        "GET",
        "/api/v1/artist-identity-groups/group",
        serde_json::Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["group"]["credited_photo_artist_id"], artist.as_str());
    assert!(!detail.to_string().contains("owned-artist-0001"));
    assert!(!list.to_string().contains("owned-artist-0001"));

    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/artist-identity-groups/group",
        serde_json::Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
