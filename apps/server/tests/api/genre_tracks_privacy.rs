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
    let operations = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let (status, acknowledged) = bounded_call(
        app,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": device,
            "operations": [
                {"id": operations[0], "action": {"kind": "recording_title_set", "recording_id": recording, "title": title}},
                {"id": operations[1], "action": {"kind": "recording_artist_set", "recording_id": recording, "artist": artist}},
                {"id": operations[2], "action": {"kind": "like_set", "recording_id": recording, "liked": true}}
            ]
        }),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(acknowledged, json!({"acknowledged": operations}));
}

fn fado_recordings(recordings: &Value) -> Vec<Value> {
    recordings["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|recording| {
            recording["inherited_title"] == "Owned Fado A"
                || recording["inherited_title"] == "Owned Fado B"
        })
        .cloned()
        .collect()
}

fn recordings_in_id_order(recordings: &Value, baselines: &[Value]) -> Vec<Value> {
    let items = recordings["items"].as_array().unwrap();
    baselines
        .iter()
        .map(|baseline| {
            items
                .iter()
                .find(|recording| recording["id"] == baseline["id"])
                .unwrap()
                .clone()
        })
        .collect()
}

async fn current_fado_recordings(
    app: &axum::Router,
    token: &str,
    baselines: &[Value],
) -> Vec<Value> {
    let (status, recordings) = bounded_call(
        app,
        "GET",
        "/api/v1/recordings?sort=title&limit=200&offset=0",
        Value::Null,
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    recordings_in_id_order(&recordings, baselines)
}

#[tokio::test]
async fn genre_track_pages_hydrate_private_metadata_and_likes_for_each_account() {
    let fixture = genre_fixture().await;
    let (member_token, admin_baseline, member_baseline) = hang_guard(
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
            let (status, admin_recordings) = call(
                &fixture.app,
                "GET",
                "/api/v1/recordings?sort=title&limit=200&offset=0",
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let (status, member_recordings) = call(
                &fixture.app,
                "GET",
                "/api/v1/recordings?sort=title&limit=200&offset=0",
                Value::Null,
                Some(&member_token),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let admin_baseline = fado_recordings(&admin_recordings);
            let member_baseline = fado_recordings(&member_recordings);
            assert_eq!(admin_baseline.len(), 2);
            assert_eq!(member_baseline.len(), 2);
            for (index, title) in ["Owned Fado A", "Owned Fado B"].into_iter().enumerate() {
                assert_eq!(admin_baseline[index]["inherited_title"], title);
                assert_eq!(member_baseline[index]["inherited_title"], title);
                assert_eq!(admin_baseline[index]["id"], member_baseline[index]["id"]);
                assert_eq!(admin_baseline[index]["title"], title);
                assert_eq!(member_baseline[index]["title"], title);
                assert_eq!(admin_baseline[index]["artist"], "Owned genre access artist");
                assert_eq!(member_baseline[index]["artist"], "Owned genre access artist");
                assert_eq!(admin_baseline[index]["inherited_artist"], "Owned genre access artist");
                assert_eq!(member_baseline[index]["inherited_artist"], "Owned genre access artist");
                assert_eq!(admin_baseline[index]["liked"], false);
                assert_eq!(member_baseline[index]["liked"], false);
            }
            (member_token, admin_baseline, member_baseline)
        }),
    )
    .await;
    let first_id = admin_baseline[0]["id"].as_str().unwrap();
    let second_id = admin_baseline[1]["id"].as_str().unwrap();
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

    let admin_projection =
        current_fado_recordings(&fixture.app, &fixture.admin, &admin_baseline).await;
    let member_projection =
        current_fado_recordings(&fixture.app, &member_token, &member_baseline).await;
    assert_eq!(admin_projection[0]["title"], "Admin private Fado title");
    assert_eq!(admin_projection[0]["artist"], "Admin private performer");
    assert_eq!(admin_projection[0]["liked"], true);
    for index in 0..2 {
        assert_eq!(
            admin_projection[index]["inherited_title"],
            admin_baseline[index]["inherited_title"]
        );
        assert_eq!(
            admin_projection[index]["inherited_artist"],
            admin_baseline[index]["inherited_artist"]
        );
        assert_eq!(
            member_projection[index]["inherited_title"],
            member_baseline[index]["inherited_title"]
        );
        assert_eq!(
            member_projection[index]["inherited_artist"],
            member_baseline[index]["inherited_artist"]
        );
    }
    assert_eq!(admin_projection[1]["title"], admin_baseline[1]["title"]);
    assert_eq!(admin_projection[1]["artist"], admin_baseline[1]["artist"]);
    assert_eq!(admin_projection[1]["liked"], false);
    assert_eq!(member_projection[0]["title"], member_baseline[0]["title"]);
    assert_eq!(member_projection[0]["artist"], member_baseline[0]["artist"]);
    assert_eq!(member_projection[0]["liked"], false);
    assert_eq!(member_projection[1]["title"], "Member private Fado title");
    assert_eq!(member_projection[1]["artist"], "Member private performer");
    assert_eq!(member_projection[1]["liked"], true);
    assert_ne!(admin_projection[0]["title"], member_projection[0]["title"]);
    assert_ne!(admin_projection[0]["artist"], member_projection[0]["artist"]);
    assert_ne!(admin_projection[1]["title"], member_projection[1]["title"]);
    assert_ne!(admin_projection[1]["artist"], member_projection[1]["artist"]);

    let admin_pages = two_track_pages(&fixture.app, &fixture.admin).await;
    let member_pages = two_track_pages(&fixture.app, &member_token).await;
    assert_eq!(admin_pages, admin_projection);
    assert_eq!(member_pages, member_projection);
    assert_eq!(admin_pages[0]["id"], admin_baseline[0]["id"]);
    assert_eq!(admin_pages[1]["id"], admin_baseline[1]["id"]);
    assert_eq!(member_pages[0]["id"], member_baseline[0]["id"]);
    assert_eq!(member_pages[1]["id"], member_baseline[1]["id"]);
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
    let (member_first, cursor) = one_track_page(&fixture.app, &member_token, None).await;
    let cursor = cursor.expect("two Fado recordings require a continuation cursor");
    let (admin_before, admin_cursor) = one_track_page(&fixture.app, &fixture.admin, None).await;
    assert!(admin_cursor.is_some());
    assert_eq!(member_first["id"], admin_before["id"]);
    assert_eq!(member_first["title"], admin_before["title"]);
    assert_eq!(member_first["title"], "Owned Fado A");

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
    assert_eq!(denied["error"], hito_core::Error::Unauthorized.to_string());
    assert!(denied.get("items").is_none());

    let (admin_after, cursor_after) = one_track_page(&fixture.app, &fixture.admin, None).await;
    assert_eq!(admin_after, admin_before);
    assert_eq!(cursor_after, admin_cursor);
}
