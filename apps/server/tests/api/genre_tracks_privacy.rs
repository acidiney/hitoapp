use crate::{
    genre_access_fixture::{
        assert_private_json, bounded_call, create_member, genre_fixture, track_request, tracks_uri,
    },
    support::call,
};
use axum::http::StatusCode;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};
use uuid::Uuid;

async fn one_track_page(
    app: &axum::Router,
    token: &str,
    after: Option<&str>,
) -> (Value, Option<String>) {
    let (status, content_type, cache_control, bytes) =
        track_request(app, &tracks_uri("fado", Some(1), after), Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let page: Value = serde_json::from_slice(&bytes).unwrap();
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    (
        items[0].clone(),
        page["next_after"].as_str().map(str::to_owned),
    )
}

async fn two_track_pages(app: &axum::Router, token: &str) -> Vec<Value> {
    let (first, cursor) = one_track_page(app, token, None).await;
    let cursor = cursor.expect("two Fado recordings require a continuation cursor");
    let (second, terminal) = one_track_page(app, token, Some(&cursor)).await;
    assert!(terminal.is_none());
    vec![first, second]
}

async fn set_personal_recording(
    app: &axum::Router,
    token: &str,
    device: &str,
    recording: &str,
    title: &str,
    artist: &str,
) {
    let (status, _) = bounded_call(
        app,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": device,
            "operations": [
                {"id": Uuid::new_v4(), "action": {"kind": "recording_title_set", "recording_id": recording, "title": title}},
                {"id": Uuid::new_v4(), "action": {"kind": "recording_artist_set", "recording_id": recording, "artist": artist}},
                {"id": Uuid::new_v4(), "action": {"kind": "like_set", "recording_id": recording, "liked": true}}
            ]
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

fn personalized(base: &Value, title: &str, artist: &str, liked: bool) -> Value {
    let mut recording = base.clone();
    recording["title"] = json!(title);
    recording["artist"] = json!(artist);
    recording["liked"] = json!(liked);
    recording
}

#[tokio::test]
async fn genre_track_pages_hydrate_private_metadata_and_likes_for_each_account() {
    let fixture = genre_fixture().await;
    let (member_token, baseline) = hang_guard(
        "genre privacy account, grant and recording baseline setup",
        Box::pin(async {
            let (member_id, member_token) = create_member(&fixture.app, &fixture.admin).await;
            let access = format!(
                "/api/v1/libraries/{}/members/{member_id}",
                fixture.library
            );
            let (status, _) = bounded_call(
                &fixture.app,
                "PUT",
                &access,
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            let (status, recordings) = call(
                &fixture.app,
                "GET",
                "/api/v1/recordings?sort=title&limit=200&offset=0",
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let fado: Vec<Value> = recordings["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|recording| {
                    recording["title"] == "Owned Fado A" || recording["title"] == "Owned Fado B"
                })
                .cloned()
                .collect();
            assert_eq!(fado.len(), 2);
            assert!(fado[0]["title"] == "Owned Fado A");
            assert!(fado[1]["title"] == "Owned Fado B");
            (member_token, fado)
        }),
    )
    .await;
    let first_id = baseline[0]["id"].as_str().unwrap();
    let second_id = baseline[1]["id"].as_str().unwrap();
    set_personal_recording(
        &fixture.app,
        &fixture.admin,
        "genre-admin-private-state",
        first_id,
        "Admin private Fado title",
        "Admin private performer",
    )
    .await;
    set_personal_recording(
        &fixture.app,
        &member_token,
        "genre-member-private-state",
        second_id,
        "Member private Fado title",
        "Member private performer",
    )
    .await;

    let admin_pages = two_track_pages(&fixture.app, &fixture.admin).await;
    let member_pages = two_track_pages(&fixture.app, &member_token).await;
    assert_eq!(admin_pages[0]["id"], baseline[0]["id"]);
    assert_eq!(admin_pages[1]["id"], baseline[1]["id"]);
    assert_eq!(member_pages[0]["id"], baseline[0]["id"]);
    assert_eq!(member_pages[1]["id"], baseline[1]["id"]);
    assert_eq!(
        admin_pages,
        vec![
            personalized(
                &baseline[0],
                "Admin private Fado title",
                "Admin private performer",
                true,
            ),
            personalized(
                &baseline[1],
                "Owned Fado B",
                "Owned genre access artist",
                false,
            ),
        ]
    );
    assert_eq!(
        member_pages,
        vec![
            personalized(
                &baseline[0],
                "Owned Fado A",
                "Owned genre access artist",
                false,
            ),
            personalized(
                &baseline[1],
                "Member private Fado title",
                "Member private performer",
                true,
            ),
        ]
    );
}

#[tokio::test]
async fn deleted_member_cannot_continue_a_genre_track_cursor() {
    let fixture = genre_fixture().await;
    let (member_id, member_token) = hang_guard(
        "genre deletion account and library grant setup",
        Box::pin(async {
            let (member_id, member_token) = create_member(&fixture.app, &fixture.admin).await;
            let access = format!(
                "/api/v1/libraries/{}/members/{member_id}",
                fixture.library
            );
            let (status, _) = bounded_call(
                &fixture.app,
                "PUT",
                &access,
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            (member_id, member_token)
        }),
    )
    .await;
    let (first, cursor) = one_track_page(&fixture.app, &member_token, None).await;
    let cursor = cursor.expect("two Fado recordings require a continuation cursor");
    assert_eq!(first["title"], "Owned Fado A");

    let (status, _) = bounded_call(
        &fixture.app,
        "DELETE",
        &format!("/api/v1/users/{member_id}"),
        json!({"username":"genre-member"}),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _, denied_bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), Some(&cursor)),
        Some(&member_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let denied: Value = serde_json::from_slice(&denied_bytes).unwrap();
    assert!(denied.get("items").is_none());

    let (admin_first, admin_cursor) = one_track_page(&fixture.app, &fixture.admin, None).await;
    assert_eq!(admin_first, first);
    assert!(admin_cursor.is_some());
}
