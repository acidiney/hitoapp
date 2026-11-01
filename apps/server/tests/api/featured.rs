use crate::support::{admin, call, services};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::featured_service::{FeaturedServer, Reply, item};
use hito_core::{InstallPlugin, OfferPluginService, PluginInstallation, Services};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SHELF: &str = "/api/v1/discover/featured";

struct Setup {
    _dir: tempfile::TempDir,
    hito: Services,
    app: Router,
    token: String,
}

async fn start() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    Setup {
        _dir: dir,
        hito,
        app,
        token,
    }
}

async fn connect(setup: &Setup, server: &FeaturedServer) -> PluginInstallation {
    let actor = setup
        .hito
        .identity
        .authenticate(&setup.token)
        .await
        .unwrap();
    let service = setup
        .hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned featured".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    setup
        .hito
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
        .unwrap()
}

async fn artwork(
    app: &Router,
    token: Option<&str>,
    query: &str,
    id: &str,
) -> (StatusCode, String, Vec<u8>) {
    let mut request =
        Request::builder().uri(format!("/api/v1/plugins/{id}/featured-artwork{query}"));
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, content_type, bytes.to_vec())
}

#[tokio::test]
async fn the_shelf_needs_a_session_and_is_unsupported_without_a_featured_provider() {
    let setup = start().await;
    assert_eq!(
        call(&setup.app, "GET", SHELF, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, shelf) = call(&setup.app, "GET", SHELF, Value::Null, Some(&setup.token)).await;
    assert_eq!(
        (status, shelf),
        (StatusCode::OK, json!({"status": "unsupported"}))
    );
}

#[tokio::test]
async fn the_shelf_lists_provider_items_and_hides_behind_a_status_when_the_provider_fails() {
    let setup = start().await;
    let server = FeaturedServer::start().await;
    let installation = connect(&setup, &server).await;
    server.reply(Reply::Items(vec![item("a"), item("b")]));

    let (status, shelf) = call(&setup.app, "GET", SHELF, Value::Null, Some(&setup.token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shelf["status"], "available");
    assert_eq!(shelf["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        shelf["items"][0]["installation_id"],
        installation.id.as_str()
    );
    assert_eq!(shelf["items"][0]["title"], "Featured a");
    assert_eq!(
        shelf["items"][0]["reference"],
        "https://provider.example/playlist/a"
    );
    let requests = server.featured_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body, json!({"limit": 12}));
    assert!(requests[0].headers.get("cookie").is_none());
    assert!(requests[0].headers.get("authorization").is_none());

    let failing = start().await;
    let broken = FeaturedServer::start().await;
    connect(&failing, &broken).await;
    broken.reply(Reply::Status(StatusCode::BAD_GATEWAY));
    let (status, shelf) = call(
        &failing.app,
        "GET",
        SHELF,
        Value::Null,
        Some(&failing.token),
    )
    .await;
    assert_eq!(
        (status, shelf),
        (StatusCode::OK, json!({"status": "unavailable"}))
    );
}

#[tokio::test]
async fn automatic_service_search_off_returns_disabled_without_contacting_the_provider() {
    let setup = start().await;
    let server = FeaturedServer::start().await;
    connect(&setup, &server).await;
    server.reply(Reply::Items(vec![item("a")]));
    let (status, _) = call(
        &setup.app,
        "PUT",
        "/api/v1/metadata/automation",
        json!({"automatic_service_search": false}),
        Some(&setup.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, shelf) = call(&setup.app, "GET", SHELF, Value::Null, Some(&setup.token)).await;
    assert_eq!(shelf, json!({"status": "disabled"}));
    assert!(server.featured_requests().is_empty());
}

#[tokio::test]
async fn featured_artwork_is_authenticated_checked_and_private_to_the_connection_owner() {
    let setup = start().await;
    let server = FeaturedServer::start().await;
    let installation = connect(&setup, &server).await;
    let query = format!("?revision={}&artwork=art-a", installation.revision);

    let (status, kind, bytes) = artwork(&setup.app, None, &query, &installation.id).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(kind.is_empty() || kind.contains("json"));
    assert!(bytes.is_empty() || !bytes.starts_with(b"\x89PNG"));

    let (status, kind, bytes) =
        artwork(&setup.app, Some(&setup.token), &query, &installation.id).await;
    assert_eq!((status, kind.as_str()), (StatusCode::OK, "image/png"));
    assert!(bytes.starts_with(b"\x89PNG"));

    for bad in [
        "",
        "?revision=1",
        "?artwork=art-a",
        "?revision=x&artwork=a",
        "?revision=1&artwork=a&extra=1",
    ] {
        let (status, _, _) = artwork(&setup.app, Some(&setup.token), bad, &installation.id).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let stale = format!("?revision={}&artwork=art-a", installation.revision + 1);
    assert_eq!(
        artwork(&setup.app, Some(&setup.token), &stale, &installation.id)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let unknown = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        artwork(&setup.app, Some(&setup.token), &query, &unknown)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    call(
        &setup.app,
        "POST",
        "/api/v1/users",
        json!({"username": "listener", "password": "featured listener password"}),
        Some(&setup.token),
    )
    .await;
    let (_, session) = call(
        &setup.app,
        "POST",
        "/api/v1/login",
        json!({"username": "listener", "password": "featured listener password"}),
        None,
    )
    .await;
    let listener = session["token"].as_str().unwrap();
    assert_eq!(
        artwork(&setup.app, Some(listener), &query, &installation.id)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, shelf) = call(&setup.app, "GET", SHELF, Value::Null, Some(listener)).await;
    assert_eq!(shelf, json!({"status": "unsupported"}));
}
