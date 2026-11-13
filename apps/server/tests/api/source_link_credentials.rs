use super::{Setup, connect, fixture};
use crate::account::{Account, exchange};
use axum::http::{HeaderMap, Method, StatusCode};
use hito_core::test_support::{
    hang_guard::hang_guard,
    source_link_service::{Answer, SourceLinkServer},
};
use serde_json::{Value, json};
use uuid::Uuid;

const REFERENCE: &str = "https://links.example/owned/private-credential-link?token=owned-link-token";

struct Observed {
    response: Value,
    expected_stamp: Value,
    plugins: Value,
    expected_plugins: Value,
}

struct Context {
    owner: Account,
    peer: Account,
    connection: Value,
    peer_connection: Value,
}

#[tokio::test]
async fn a_current_source_link401_identifies_only_its_approved_connection() {
    let observed = fixture::run(async |owned| {
        let server = owned.server().await;
        let setup = owned.setup.as_ref().unwrap();
        let context = hang_guard(
            "source-link credential tracer accounts and current generation",
            Box::pin(prepare(setup, &server)),
        )
        .await;
        exercise(setup, &server, &context).await
    })
    .await;
    assert_eq!(
        observed.response.get("plugin_connection"),
        Some(&observed.expected_stamp),
        "A checked source-link401 must identify its current approved installation after owned retirement"
    );
    assert_eq!(
        observed.response,
        json!({
            "error": "Update this plugin connection's service credentials in Settings.",
            "code": "plugin_credentials_required",
            "plugin_connection": observed.expected_stamp,
        })
    );
    assert_eq!(observed.plugins, observed.expected_plugins);
}

async fn prepare(setup: &Setup, server: &SourceLinkServer) -> Context {
    let installation = connect(setup, server).await;
    let owner = Account::login(
        &setup.app,
        "admin",
        "administrator test password",
    )
    .await;
    let (status, refreshed) = owner
        .request(
            &setup.app,
            "POST",
            &format!("/api/v1/plugins/{}/refresh", installation.id),
            json!({"revision": installation.revision}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(refreshed["kind"], "updated");
    let connection = refreshed["installation"].clone();
    assert_eq!(connection["id"], installation.id);
    assert_eq!(connection["revision"], installation.revision + 1);
    let (status, _) = owner
        .request(
            &setup.app,
            "POST",
            "/api/v1/users",
            json!({
                "username": "source-link-credential-peer",
                "password": "owned source link credential peer password",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        &setup.app,
        "source-link-credential-peer",
        "owned source link credential peer password",
    )
    .await;
    let offered = plugins(setup, &owner).await;
    let service = offered["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["id"] == connection["service_id"])
        .unwrap();
    let (status, peer_connection) = peer
        .request(
            &setup.app,
            "POST",
            "/api/v1/plugins",
            json!({
                "service_id": service["id"],
                "service_revision": service["revision"],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(peer_connection["id"], connection["id"]);
    Context {
        owner,
        peer,
        connection,
        peer_connection,
    }
}

async fn exercise(setup: &Setup, server: &SourceLinkServer, context: &Context) -> Observed {
    let before = plugins(setup, &context.owner).await;
    let peer_before = plugins(setup, &context.peer).await;
    let row = installation(&before, context.connection["id"].as_str().unwrap());
    assert_eq!(row["revision"], context.connection["revision"]);
    assert_eq!(row["health"], json!({"state": "ok", "reason": null, "since": null}));
    assert_eq!(
        peer_before["installations"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        peer_before["installations"][0]["id"],
        context.peer_connection["id"]
    );
    let original = snapshot(setup, &context.owner).await;
    let peer_original = snapshot_peer(setup, &context.peer).await;
    let health_reads = server.health_requests();
    assert_eq!(health_reads, 2);
    let path = format!("/api/v1/recordings/{}/sources/lookup", setup.recording);
    let calls_before = server.lookup_requests().len();
    for (id, revision) in [
        (json!(context.connection["id"]), json!(context.connection["revision"])),
        (
            context.peer_connection["id"].clone(),
            context.peer_connection["revision"].clone(),
        ),
    ] {
        let (status, headers, denied) = exchange(
            &setup.app,
            "POST",
            &path,
            json!({
                "installation_id": id,
                "installation_revision": revision,
                "reference": REFERENCE,
            }),
            context.peer.headers(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        private(&headers);
        assert_eq!(denied, json!({"error": "Resource not found"}));
        assert_eq!(server.lookup_requests().len(), calls_before);
    }
    server.answer(REFERENCE, Answer::Status(StatusCode::UNAUTHORIZED));
    let (status, headers, response) = exchange(
        &setup.app,
        "POST",
        &path,
        json!({
            "installation_id": context.connection["id"],
            "installation_revision": context.connection["revision"],
            "reference": REFERENCE,
        }),
        context.owner.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    private(&headers);
    assert_eq!(response["code"], "plugin_credentials_required");
    assert_eq!(
        response["error"],
        "Update this plugin connection's service credentials in Settings."
    );
    assert!(response.as_object().unwrap().keys().all(|key| {
        matches!(key.as_str(), "error" | "code" | "plugin_connection")
    }));
    let encoded = response.to_string();
    assert!(!encoded.contains(REFERENCE));
    assert!(!encoded.contains("owned-link-token"));
    assert_eq!(snapshot(setup, &context.owner).await, original);
    assert_eq!(snapshot_peer(setup, &context.peer).await, peer_original);
    assert_eq!(plugins(setup, &context.peer).await, peer_before);
    let calls = server.lookup_requests();
    assert_eq!(calls.len(), calls_before + 1);
    let request = &calls[calls_before];
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.json(), json!({"reference": REFERENCE}));
    assert_eq!(request.protocol().as_deref(), Some("1"));
    assert!(request.query.is_none());
    assert!(request.headers.get("authorization").is_none());
    assert!(request.headers.get("cookie").is_none());
    assert_eq!(server.detail_requests(), 0);
    assert_eq!(server.source_requests(), 0);
    let plugins = plugins(setup, &context.owner).await;
    assert_eq!(server.health_requests(), health_reads);
    assert_eq!(server.unexpected(), 0);
    let mut expected_plugins = before;
    let row = expected_plugins["installations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == context.connection["id"])
        .unwrap();
    row["health"] = json!({"state": "credentials_required", "reason": null, "since": null});
    Observed {
        response,
        expected_stamp: json!({
            "installation_id": context.connection["id"],
            "installation_revision": context.connection["revision"],
        }),
        plugins,
        expected_plugins,
    }
}

async fn plugins(setup: &Setup, account: &Account) -> Value {
    let (status, headers, value) = exchange(
        &setup.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    value
}

fn installation<'a>(settings: &'a Value, id: &str) -> &'a Value {
    settings["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|installation| installation["id"] == id)
        .unwrap()
}

async fn snapshot(setup: &Setup, owner: &Account) -> Value {
    let mut state = Vec::new();
    for path in [
        "/api/v1/recordings".to_owned(),
        format!("/api/v1/recordings/{}/metadata", setup.recording),
        format!("/api/v1/recordings/{}/sources", setup.recording),
        format!("/api/v1/recordings/{}/lyrics/identities", setup.recording),
        format!("/api/v1/recordings/{}/source-choice", setup.recording),
        format!("/api/v1/recordings/{}/sources", setup.shared),
        "/api/v1/playlists".to_owned(),
        "/api/v1/activity".to_owned(),
        "/api/v1/sync?cursor=0".to_owned(),
        "/api/v1/player".to_owned(),
    ] {
        state.push(owner.get(&setup.app, &path).await);
    }
    Value::Array(state)
}

async fn snapshot_peer(setup: &Setup, peer: &Account) -> Value {
    json!({
        "recordings": peer.get(&setup.app, "/api/v1/recordings").await,
        "playlists": peer.get(&setup.app, "/api/v1/playlists").await,
        "activity": peer.get(&setup.app, "/api/v1/activity").await,
        "sync": peer.get(&setup.app, "/api/v1/sync?cursor=0").await,
        "player": peer.get(&setup.app, "/api/v1/player").await,
        "plugins": plugins(setup, peer).await,
    })
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
