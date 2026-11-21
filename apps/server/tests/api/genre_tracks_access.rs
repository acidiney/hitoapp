use crate::{
    genre_access_fixture::{
        assert_private_json, bounded_call, create_member, genre_fixture, track_request, tracks_uri,
    },
    support::call,
};
use axum::http::StatusCode;
use hito_core::test_support::hang_guard::hang_guard;
use serde_json::{Value, json};

#[tokio::test]
async fn genre_track_reads_use_the_present_account_and_obey_lock_and_revocation() {
    let fixture = genre_fixture().await;
    let uri = tracks_uri("fado", Some(1), None);
    let (status, _, _, _) = track_request(&fixture.app, &uri, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (_member_id, member_token) = hang_guard(
        "genre member account setup",
        Box::pin(create_member(&fixture.app, &fixture.admin)),
    )
    .await;
    let (status, _, _, _) = track_request(
        &fixture.app,
        &uri,
        Some(&member_token),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, content_type, cache_control, bytes) =
        track_request(&fixture.app, &uri, None, Some(&fixture.admin)).await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let cursor = page["next_after"].as_str().unwrap().to_owned();

    let (status, _) = bounded_call(
        &fixture.app,
        "PUT",
        "/api/v1/account/pin",
        json!({"password":"administrator test password","pin":"1234","idle_minutes":0}),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bounded_call(
        &fixture.app,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let after_uri = tracks_uri("fado", Some(1), Some(&cursor));
    let (status, _, _, bytes) =
        track_request(&fixture.app, &after_uri, None, Some(&fixture.admin)).await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["code"],
        "locked"
    );

    let (status, _) = bounded_call(
        &fixture.app,
        "POST",
        "/api/v1/account/unlock",
        json!({"pin":"1234"}),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = bounded_call(
        &fixture.app,
        "POST",
        "/api/v1/logout",
        Value::Null,
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _, _) = track_request(
        &fixture.app,
        &after_uri,
        Some(&fixture.admin),
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn genre_track_cursors_limits_and_encoded_keys_keep_their_scopes() {
    let fixture = genre_fixture().await;
    let (status, member_token) = hang_guard(
        "genre member access setup",
        Box::pin(async {
            let (member_id, member_token) = create_member(&fixture.app, &fixture.admin).await;
            let access = format!("/api/v1/libraries/{}/members/{member_id}", fixture.library);
            let (status, _) = call(
                &fixture.app,
                "PUT",
                &access,
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            (status, member_token)
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _, first_bytes) = track_request(
        &fixture.app,
        &tracks_uri("FADO", Some(1), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first: Value = serde_json::from_slice(&first_bytes).unwrap();
    let cursor = first["next_after"].as_str().unwrap().to_owned();

    let (status, _, _, lower_bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(0), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let lower_page: Value = serde_json::from_slice(&lower_bytes).unwrap();
    assert_eq!(lower_page["items"].as_array().unwrap().len(), 1);
    assert!(lower_page["next_after"].as_str().is_some());

    let (status, _, _, _) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), Some("not-a-valid-cursor")),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _, _) = track_request(
        &fixture.app,
        &tracks_uri("jazz", Some(1), Some(&cursor)),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _, _) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), Some(&cursor)),
        Some(&member_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, _, _) = track_request(
        &fixture.app,
        &tracks_uri("no such genre", Some(1), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let encoded_key = tracks_uri("CAFÉ + /?", Some(10), None);
    assert!(
        encoded_key.contains("%2B") && encoded_key.contains("%2F") && encoded_key.contains("%3F")
    );
    let (status, content_type, cache_control, bytes) =
        track_request(&fixture.app, &encoded_key, Some(&fixture.admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let unicode_page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(unicode_page["items"].as_array().unwrap().len(), 1);
    assert!(unicode_page["next_after"].is_null());
}

#[tokio::test]
async fn genre_track_continuation_rechecks_revoked_library_access() {
    let fixture = genre_fixture().await;
    let (status, member_id, member_token) = hang_guard(
        "genre member account and library grant setup",
        Box::pin(async {
            let (member_id, member_token) = create_member(&fixture.app, &fixture.admin).await;
            let access = format!("/api/v1/libraries/{}/members/{member_id}", fixture.library);
            let (status, _) = bounded_call(
                &fixture.app,
                "PUT",
                &access,
                Value::Null,
                Some(&fixture.admin),
            )
            .await;
            (status, member_id, member_token)
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, content_type, cache_control, bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), None),
        Some(&member_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let cursor = page["next_after"].as_str().unwrap().to_owned();

    let access = format!("/api/v1/libraries/{}/members/{member_id}", fixture.library);
    let (status, _) = bounded_call(
        &fixture.app,
        "DELETE",
        &access,
        Value::Null,
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _, bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), Some(&cursor)),
        Some(&member_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        json!({"error": hito_core::Error::NotFound.to_string()})
    );

    let (status, content_type, cache_control, bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), None),
        Some(&fixture.admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    let admin_page: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(admin_page["items"].as_array().unwrap().len(), 1);
    assert!(admin_page["next_after"].as_str().is_some());

    let (status, _) = bounded_call(
        &fixture.app,
        "PUT",
        &access,
        Value::Null,
        Some(&fixture.admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, content_type, cache_control, bytes) = track_request(
        &fixture.app,
        &tracks_uri("fado", Some(1), None),
        Some(&member_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_private_json(content_type, cache_control);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
