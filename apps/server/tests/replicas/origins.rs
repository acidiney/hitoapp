use super::fixture::{Host, Library};
use hito_core::{Action, Operation};
use uuid::Uuid;

#[tokio::test]
async fn native_replica_uses_an_alias_when_the_servers_browser_origin_is_different() {
    let remote = Host::start().await;
    let local = Library::new("owned-alias-client").await;
    let connection=local.services.replicas.connect(&local.actor,remote.request("admin")).await.expect("A native replica must connect through an explicit LAN alias without pretending it is the browser origin");
    let session = local
        .services
        .replicas
        .activate(&local.actor, connection.id)
        .await
        .unwrap();
    let playlist = Uuid::new_v4();
    local
        .services
        .replicas
        .enqueue(
            &local.actor,
            &session,
            Operation {
                id: Uuid::new_v4(),
                action: Action::PlaylistCreate {
                    playlist_id: playlist,
                    name: "Owned alias playlist".into(),
                },
            },
        )
        .await
        .unwrap();
    let result = local
        .services
        .replicas
        .synchronize(&local.actor, &session)
        .await
        .unwrap();
    assert_eq!(result.pushed, 1);
    assert_eq!(result.pending, 0);
    let playlists = remote
        .library
        .services
        .collection
        .playlists(&remote.library.actor)
        .await
        .unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].id, playlist.to_string());
    local.services.replicas.close().await.unwrap();
    remote.close().await;
}

#[tokio::test]
async fn browser_requests_with_an_unlisted_origin_are_still_rejected() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let library = Library::new("owned-browser-origin-host").await;
    let app = hito_server::router_with_config(
        library.services.clone(),
        hito_server::Config {
            origin: "https://canonical-hito.invalid".into(),
            ..Default::default()
        },
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/login")
                .header("origin", "https://untrusted-browser.invalid")
                .header("x-hito-request", "1")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&super::fixture::credentials("admin")).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    library.services.replicas.close().await.unwrap();
}
