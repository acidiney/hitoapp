#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

#[path = "api/playlist_fixture.rs"]
mod playlist_fixture;

#[path = "change_events/producers.rs"]
mod producers;

use hito_core::test_support::content_service as provider_fixture;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::time::Duration;
use support::{admin, app, call, indexed_app};
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "administrator test password";

async fn open(app: &Router, token: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/changes/events")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn stream(app: &Router, token: &str) -> Body {
    let response = open(app, token).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let mut body = response.into_body();
    assert_eq!(
        notice(&mut body).await,
        json!({"kind":"resync","reason":"connected"})
    );
    body
}

async fn notice(body: &mut Body) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .expect("a change notice must arrive")
            .expect("the change stream must stay open")
            .unwrap()
            .into_data()
            .unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        if let Some(data) = text.strip_prefix("data: ") {
            return serde_json::from_str(data.trim()).unwrap();
        }
    }
}

async fn ended(body: &mut Body) {
    let next = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .expect("the change stream must end promptly");
    assert!(next.is_none(), "the change stream must end, not send more");
}

async fn quiet(body: &mut Body) {
    assert!(
        tokio::time::timeout(Duration::from_millis(200), body.frame())
            .await
            .is_err(),
        "no change notice may arrive"
    );
}

async fn sync(app: &Router, token: &str, actions: Vec<Value>) -> StatusCode {
    let operations: Vec<Value> = actions
        .into_iter()
        .map(|action| json!({"id": Uuid::new_v4(), "action": action}))
        .collect();
    call(
        app,
        "POST",
        "/api/v1/sync",
        json!({"device_id": "change-events", "operations": operations}),
        Some(token),
    )
    .await
    .0
}

fn create(playlist: &str) -> Value {
    json!({"kind":"playlist_create","playlist_id":playlist,"name":"Notice"})
}

fn playlist_notice(playlist: &str) -> Value {
    json!({"kind":"playlist","playlist_id":playlist})
}

async fn login(app: &Router, username: &str) -> String {
    call(
        app,
        "POST",
        "/api/v1/login",
        json!({"username": username, "password": PASSWORD}),
        None,
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn guest(app: &Router, owner: &str) -> String {
    assert_eq!(
        call(
            app,
            "POST",
            "/api/v1/users",
            json!({"username": "guest", "password": PASSWORD}),
            Some(owner),
        )
        .await
        .0,
        StatusCode::CREATED
    );
    login(app, "guest").await
}

#[tokio::test]
async fn the_stream_needs_a_session() {
    let (_dir, app) = app().await;
    admin(&app).await;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/changes/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_account_receives_its_own_committed_notices_and_never_another_accounts() {
    let (_dir, app) = app().await;
    let owner = admin(&app).await;
    let guest = guest(&app, &owner).await;
    let mut owner_stream = stream(&app, &owner).await;
    let mut guest_stream = stream(&app, &guest).await;

    let private = Uuid::new_v4().to_string();
    assert_eq!(
        sync(&app, &guest, vec![create(&private)]).await,
        StatusCode::OK
    );
    assert_eq!(notice(&mut guest_stream).await, playlist_notice(&private));
    assert_eq!(notice(&mut guest_stream).await, json!({"kind":"playlists"}));

    let own = Uuid::new_v4().to_string();
    assert_eq!(sync(&app, &owner, vec![create(&own)]).await, StatusCode::OK);
    assert_eq!(notice(&mut owner_stream).await, playlist_notice(&own));
    let (_, playlists) = call(&app, "GET", "/api/v1/playlists", Value::Null, Some(&owner)).await;
    assert_eq!(playlists[0]["id"], own.as_str());
    assert_eq!(notice(&mut owner_stream).await, json!({"kind":"playlists"}));
    quiet(&mut owner_stream).await;
    quiet(&mut guest_stream).await;
}

#[tokio::test]
async fn a_rolled_back_batch_or_rejected_command_emits_nothing() {
    let (_dir, app, token, recording) = indexed_app().await;
    let mut changes = stream(&app, &token).await;

    let rolled_back = Uuid::new_v4().to_string();
    let missing = Uuid::new_v4().to_string();
    assert_ne!(
        sync(
            &app,
            &token,
            vec![
                create(&rolled_back),
                json!({"kind":"entry_insert","playlist_id":missing,"entry_id":Uuid::new_v4(),"recording_id":recording,"position":0}),
            ],
        )
        .await,
        StatusCode::OK
    );
    assert_ne!(
        sync(
            &app,
            &token,
            vec![json!({"kind":"like_set","recording_id":Uuid::new_v4(),"liked":true})],
        )
        .await,
        StatusCode::OK
    );
    quiet(&mut changes).await;

    assert_eq!(
        sync(
            &app,
            &token,
            vec![json!({"kind":"like_set","recording_id":recording,"liked":true})],
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        notice(&mut changes).await,
        json!({"kind":"recording","recording_id":recording})
    );
    quiet(&mut changes).await;
}

#[tokio::test]
async fn the_stream_ends_on_sign_out() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let mut changes = stream(&app, &token).await;
    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    ended(&mut changes).await;
    assert_eq!(open(&app, &token).await.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_stream_ends_when_its_session_is_revoked_and_other_sessions_keep_theirs() {
    let (_dir, app) = app().await;
    let first = admin(&app).await;
    let second = login(&app, "admin").await;
    let third = login(&app, "admin").await;
    let mut first_changes = stream(&app, &first).await;
    let mut second_changes = stream(&app, &second).await;
    let mut third_changes = stream(&app, &third).await;

    let (_, sessions) = call(
        &app,
        "GET",
        "/api/v1/account/sessions",
        Value::Null,
        Some(&second),
    )
    .await;
    let current = sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["current"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/account/sessions/{current}"),
            Value::Null,
            Some(&first),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    ended(&mut second_changes).await;
    quiet(&mut first_changes).await;

    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/account/sessions",
            Value::Null,
            Some(&first),
        )
        .await
        .0,
        StatusCode::OK
    );
    ended(&mut third_changes).await;
    let playlist = Uuid::new_v4().to_string();
    assert_eq!(
        sync(&app, &first, vec![create(&playlist)]).await,
        StatusCode::OK
    );
    assert_eq!(notice(&mut first_changes).await, playlist_notice(&playlist));
}

#[tokio::test]
async fn a_slow_consumer_is_told_to_resync_instead_of_buffering_without_limit() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let mut changes = stream(&app, &token).await;
    let playlists: Vec<String> = (0..40).map(|_| Uuid::new_v4().to_string()).collect();
    assert_eq!(
        sync(
            &app,
            &token,
            playlists.iter().map(|id| create(id)).collect()
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        notice(&mut changes).await,
        json!({"kind":"resync","reason":"lagged"})
    );
    let mut retained = 0;
    loop {
        let value = notice(&mut changes).await;
        retained += 1;
        if value == playlist_notice(&playlists[39]) {
            break;
        }
    }
    assert!(retained <= 32, "only a bounded window may be retained");
    let next = Uuid::new_v4().to_string();
    assert_eq!(
        sync(&app, &token, vec![create(&next)]).await,
        StatusCode::OK
    );
    assert_eq!(notice(&mut changes).await, playlist_notice(&next));
}

#[tokio::test]
async fn each_account_holds_a_bounded_number_of_streams_and_a_reconnect_starts_with_resync() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let mut open_streams = Vec::new();
    for _ in 0..8 {
        open_streams.push(stream(&app, &token).await);
    }
    assert_eq!(
        open(&app, &token).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    open_streams.pop();
    let mut reconnected = stream(&app, &token).await;
    let playlist = Uuid::new_v4().to_string();
    assert_eq!(
        sync(&app, &token, vec![create(&playlist)]).await,
        StatusCode::OK
    );
    assert_eq!(notice(&mut reconnected).await, playlist_notice(&playlist));
    for mut other in open_streams {
        assert_eq!(notice(&mut other).await, playlist_notice(&playlist));
    }
}

async fn held_with_a_queued_private_notice(app: &Router, token: &str) -> Body {
    let changes = stream(app, token).await;
    let playlist = Uuid::new_v4().to_string();
    assert_eq!(
        sync(app, token, vec![create(&playlist)]).await,
        StatusCode::OK
    );
    changes
}

#[tokio::test]
async fn a_held_stream_delivers_no_queued_notice_after_sign_out() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    let mut changes = held_with_a_queued_private_notice(&app, &token).await;
    assert_eq!(
        call(&app, "POST", "/api/v1/logout", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    ended(&mut changes).await;
}

#[tokio::test]
async fn a_held_stream_delivers_no_queued_notice_after_its_session_is_revoked() {
    let (_dir, app) = app().await;
    let first = admin(&app).await;
    let second = login(&app, "admin").await;
    let mut changes = held_with_a_queued_private_notice(&app, &second).await;
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/account/sessions",
            Value::Null,
            Some(&first),
        )
        .await
        .0,
        StatusCode::OK
    );
    ended(&mut changes).await;
}

#[tokio::test]
async fn a_held_stream_delivers_no_queued_notice_after_its_session_is_locked() {
    let (_dir, app) = app().await;
    let token = admin(&app).await;
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": PASSWORD, "pin": "1234", "idle_minutes": 0}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut changes = held_with_a_queued_private_notice(&app, &token).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/account/lock",
            Value::Null,
            Some(&token)
        )
        .await
        .0,
        StatusCode::OK
    );
    ended(&mut changes).await;
}

async fn database(dir: &tempfile::TempDir, statement: &str, seconds: i64) {
    use sqlx::Connection;
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query(statement)
        .bind(seconds)
        .execute(&mut connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_held_stream_delivers_no_queued_notice_after_its_session_expires() {
    let (dir, app) = app().await;
    let token = admin(&app).await;
    let mut changes = held_with_a_queued_private_notice(&app, &token).await;
    database(&dir, "UPDATE sessions SET expires_at = unixepoch() - ?", 1).await;
    ended(&mut changes).await;
}

#[tokio::test]
async fn a_held_stream_delivers_no_queued_notice_after_idling_into_a_lock_and_never_defers_it() {
    let (dir, app) = app().await;
    let token = admin(&app).await;
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": PASSWORD, "pin": "1234", "idle_minutes": 5}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut changes = stream(&app, &token).await;
    let first = Uuid::new_v4().to_string();
    let second = Uuid::new_v4().to_string();
    assert_eq!(
        sync(&app, &token, vec![create(&first), create(&second)]).await,
        StatusCode::OK
    );
    let aged = "UPDATE session_locks SET active_at = active_at - ?";
    database(&dir, aged, 3 * 60).await;
    assert_eq!(notice(&mut changes).await, playlist_notice(&first));
    database(&dir, aged, 3 * 60).await;
    ended(&mut changes).await;
    assert_eq!(
        call(&app, "GET", "/api/v1/me", Value::Null, Some(&token))
            .await
            .0,
        StatusCode::LOCKED
    );
}
