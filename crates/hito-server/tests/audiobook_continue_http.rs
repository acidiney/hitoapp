#[path = "../../hito-core/tests/fixtures/audiobook_continue_library.rs"]
mod library;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
#[path = "../../hito-core/tests/support/mod.rs"]
mod support;

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use hito_core::{Action, AudiobookChapterAnchor, Credentials};
use hito_playback::Player;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const PREPARE: &str = "/api/v1/player/audiobooks/prepare";
const CANCEL: &str = "/api/v1/player/audiobooks/cancel";

#[tokio::test]
async fn fresh_private_audiobook_playback_prepares_and_cancels_without_mutating_the_current_player()
{
    let f = library::Library::start().await;
    let expected = AudiobookChapterAnchor {
        audiobook_id: f.book,
        entry_id: f.entries[3],
        recording_id: f.recordings[0],
    };
    support::synchronize(
        &f.services,
        &f.actor,
        vec![
            Action::ProgressSet {
                recording_id: f.recordings[0],
                position_ms: 750,
                speed: 0.75,
            },
            Action::AudiobookChapterSet { chapter: expected },
        ],
    )
    .await;
    let login = || Credentials {
        username: "continue-listener".into(),
        password: "private continue listener password".into(),
    };
    let session = f.services.identity.login(login()).await.unwrap();
    let other_session = f.services.identity.login(login()).await.unwrap();
    let admin = f
        .services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    assert_eq!(admin.user.id, f.admin.user().id);
    let account = Some((session.token.as_str(), session.user.id.as_str()));
    let same_account = Some((other_session.token.as_str(), other_session.user.id.as_str()));
    let foreign = Some((admin.token.as_str(), admin.user.id.as_str()));
    let (player, owner) = Player::start(f.services.clone());
    let app = hito_server::router_with_player(f.services.clone(), Default::default(), player);
    let before = f.snapshot().await;
    let path = format!("/api/v1/audiobooks/{}/playback", f.book);
    let projection = request(&app, "GET", &path, Value::Null, account).await;
    assert_eq!(projection.status, StatusCode::OK);
    assert_private(&projection);
    assert_eq!(projection.body["audiobook_id"], f.book.to_string());
    assert_eq!(projection.body["chapter"], json!(expected));
    assert_eq!(
        projection.body["progress"],
        json!({
            "recording_id": f.recordings[0], "position_ms": 750, "speed": 0.75,
        })
    );
    assert_eq!(projection.body["chapters"].as_array().unwrap().len(), 3);
    assert_eq!(
        projection.body["chapters"][1]["entry_id"],
        f.entries[2].to_string()
    );
    assert_eq!(projection.body["chapters"][1]["available"], false);
    assert_eq!(
        request(&app, "GET", &path, Value::Null, None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", &path, Value::Null, foreign)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let inspection = request(&app, "GET", "/api/v1/player", Value::Null, account).await;
    assert_eq!(inspection.status, StatusCode::OK);
    let attachment = request(
        &app,
        "POST",
        "/api/v1/player/claim",
        json!({"epoch": inspection.body["epoch"]}),
        account,
    )
    .await;
    assert_eq!(attachment.status, StatusCode::OK);
    let lease = &attachment.body["lease"];
    let command = json!({
        "id": Uuid::new_v4(), "lease": lease, "sequence": 1, "revision": 0,
        "action": {"kind": "replace", "recordings": [f.recordings[2]], "start": 0},
    });
    assert_eq!(
        request(&app, "POST", "/api/v1/player/commands", command, account)
            .await
            .status,
        StatusCode::OK
    );
    let old = request(&app, "GET", "/api/v1/player", Value::Null, account)
        .await
        .body;
    let query = json!({"lease": lease, "revision": 1, "offset": 0, "limit": 10});
    let old_queue = request(&app, "POST", "/api/v1/player/queue", query.clone(), account)
        .await
        .body;
    let input = json!({
        "id": Uuid::new_v4(), "lease": lease, "revision": old["state"]["revision"],
        "selection": old["state"]["selection"], "audiobook_id": f.book,
    });
    assert_eq!(
        request(&app, "POST", PREPARE, input.clone(), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "POST", PREPARE, input.clone(), foreign)
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, "POST", PREPARE, input.clone(), same_account)
            .await
            .status,
        StatusCode::CONFLICT
    );
    let mut invalid = input.clone();
    invalid["extra"] = json!(true);
    assert_eq!(
        request(&app, "POST", PREPARE, invalid, account)
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for field in ["lease", "revision", "selection"] {
        let mut stale = input.clone();
        stale[field] = match field {
            "lease" => json!(Uuid::new_v4()),
            "revision" => json!(0),
            _ => json!(old["state"]["selection"].as_u64().unwrap() + 1),
        };
        assert_eq!(
            request(&app, "POST", PREPARE, stale, account).await.status,
            StatusCode::CONFLICT
        );
    }
    let prepared = request(&app, "POST", PREPARE, input.clone(), account).await;
    assert_eq!(prepared.status, StatusCode::OK);
    assert_private(&prepared);
    assert_eq!(prepared.body["chapter"], json!(expected));
    assert_eq!(prepared.body["position_ms"], 750);
    assert_eq!(prepared.body["speed"], 750);
    assert_eq!(prepared.body["count"], 3);
    assert!(
        prepared.body["token"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .is_ok()
    );
    assert_eq!(
        request(&app, "GET", "/api/v1/player", Value::Null, account)
            .await
            .body,
        old
    );
    let cancel = json!({"id": input["id"], "lease": lease});
    assert_eq!(
        request(&app, "POST", CANCEL, cancel.clone(), foreign)
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, "POST", CANCEL, cancel.clone(), same_account)
            .await
            .status,
        StatusCode::CONFLICT
    );
    let mut invalid_cancel = cancel.clone();
    invalid_cancel["token"] = prepared.body["token"].clone();
    assert_eq!(
        request(&app, "POST", CANCEL, invalid_cancel, account)
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let cancelled = request(&app, "POST", CANCEL, cancel, account).await;
    assert_eq!(cancelled.status, StatusCode::NO_CONTENT);
    assert_private(&cancelled);
    assert!(cancelled.body.is_null());
    let mut commit = json!({
        "id": Uuid::new_v4(), "lease": lease, "sequence": 2, "revision": 1,
        "action": {"kind": "continue_audiobook", "token": prepared.body["token"]},
    });
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/player/commands",
            commit.clone(),
            account
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, "GET", "/api/v1/player", Value::Null, account)
            .await
            .body,
        old
    );
    assert_eq!(
        request(&app, "POST", "/api/v1/player/queue", query, account)
            .await
            .body,
        old_queue
    );
    assert_eq!(f.snapshot().await, before);
    let mut fresh = input;
    fresh["id"] = json!(Uuid::new_v4());
    let prepared = request(&app, "POST", PREPARE, fresh.clone(), account).await;
    assert_eq!(prepared.status, StatusCode::OK);
    commit["id"] = json!(Uuid::new_v4());
    commit["action"]["token"] = prepared.body["token"].clone();
    let accepted = request(
        &app,
        "POST",
        "/api/v1/player/commands",
        commit.clone(),
        account,
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(accepted.body["sequence"], 2);
    assert_eq!(accepted.body["revision"], 2);
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/player/commands",
            commit.clone(),
            account
        )
        .await
        .body,
        accepted.body
    );
    let settlement = json!({"lease": lease, "id": fresh["id"], "command": commit});
    for denied in [None, foreign, same_account] {
        assert_eq!(
            request(&app, "POST", CANCEL, settlement.clone(), denied)
                .await
                .status,
            if denied.is_none() {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::CONFLICT
            }
        );
    }
    let settled = request(&app, "POST", CANCEL, settlement.clone(), account).await;
    assert_eq!(settled.status, StatusCode::OK);
    assert_private(&settled);
    assert_eq!(settled.body, json!({"receipt": accepted.body}));
    assert_eq!(
        request(&app, "POST", CANCEL, settlement.clone(), account)
            .await
            .body,
        settled.body
    );
    let mut mismatched = settlement.clone();
    mismatched["id"] = json!(Uuid::new_v4());
    assert_eq!(
        request(&app, "POST", CANCEL, mismatched, account)
            .await
            .status,
        StatusCode::CONFLICT
    );
    let mut invalid = settlement;
    invalid["command"]["extra"] = json!(true);
    assert_eq!(
        request(&app, "POST", CANCEL, invalid, account).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let current = request(&app, "GET", "/api/v1/player", Value::Null, account)
        .await
        .body;
    assert_eq!(current["state"]["current"]["chapter"], json!(expected));
    assert_eq!(current["state"]["current_index"], 2);
    assert_eq!(current["state"]["queue_len"], 3);
    assert_eq!(current["state"]["speed"], 750);
    assert_eq!(current["state"]["desired_playing"], true);
    owner.close().await.unwrap();
    assert_eq!(f.snapshot().await, before);
    assert_eq!(std::fs::read(&f.original_path).unwrap(), f.original);
    assert!(f.server.requests("v1/audio").await.is_empty());
    assert!(f.directory.path().join("hito.db").is_file());
}

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

fn assert_private(response: &Response) {
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert!(response.headers.get("set-cookie").is_none());
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    account: Option<(&str, &str)>,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-hito-request", "1")
        .header("origin", "http://localhost:8080")
        .header("content-type", "application/json");
    if let Some((token, id)) = account {
        request = request
            .header("cookie", format!("hito_session={token}"))
            .header("x-hito-account", id);
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 262_144)
        .await
        .unwrap();
    Response {
        status,
        headers,
        body: if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    }
}
