use crate::support::{admin, call, services};
use axum::http::StatusCode;
use hito_core::test_support::support::write_wav;
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use uuid::Uuid;

const M1: &str = "11111111-1111-4111-8111-111111111111";

fn tagged(path: &std::path::Path, offset: i16, title: &str, album: &str, mbid: Option<&str>) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("C4 Pedro".into());
    tag.set_album(album.into());
    if let Some(mbid) = mbid {
        tag.insert_text(ItemKey::MusicBrainzArtistId, mbid.into());
    }
    tag.save_to_path(path, Default::default()).unwrap();
}

#[tokio::test]
async fn http_matching_applies_embedded_ids_and_serves_ranked_dismissable_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    tagged(&music.join("a.wav"), 1, "Song A", "Album A", Some(M1));
    tagged(&music.join("b.wav"), 2, "Song B", "Album B", Some(M1));
    tagged(&music.join("p.wav"), 3, "Song P", "Album A", None);
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let token = Some(token.as_str());
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, token).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        token,
    )
    .await;
    hito.storage.run_next_job().await.unwrap();

    let body = json!({"request_id": Uuid::new_v4()});
    let (status, receipt) =
        call(&app, "POST", "/api/v1/artist-matching", body.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["request_id"], body["request_id"]);
    assert_eq!(
        (
            &receipt["matched"],
            &receipt["skipped_ambiguous"],
            &receipt["skipped_declined"],
            &receipt["next_after"]
        ),
        (&json!(2), &json!(0), &json!(0), &Value::Null)
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/artist-matching", body.clone(), token).await,
        (StatusCode::OK, receipt.clone())
    );
    let mut changed = body.clone();
    changed["limit"] = json!(1);
    assert_eq!(
        call(&app, "POST", "/api/v1/artist-matching", changed, token)
            .await
            .0,
        StatusCode::CONFLICT
    );
    for (bad, expected) in [
        (
            json!({"request_id": Uuid::new_v4(), "limit": 0}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"request_id": Uuid::new_v4(), "limit": 101}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"request_id": Uuid::new_v4(), "extra": 1}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"request_id": Uuid::nil()}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        assert_eq!(
            call(&app, "POST", "/api/v1/artist-matching", bad, token)
                .await
                .0,
            expected
        );
    }
    assert_eq!(
        call(&app, "POST", "/api/v1/artist-matching", body.clone(), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    let (_, credits) = call(&app, "GET", "/api/v1/artists", Value::Null, token).await;
    let credit = credits["items"][0]["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/artists/{credit}/match-suggestions");
    let (status, found) = call(&app, "GET", &path, Value::Null, token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["artist_id"], credit);
    assert_eq!(found["dismissed"], 0);
    let items = found["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["group"]["provider_artist_id"], M1);
    assert_eq!(
        (
            &items[0]["shared_albums"],
            &items[0]["same_length_tracks"],
            &items[0]["undecided_tracks"]
        ),
        (&json!(1), &json!(0), &json!(1))
    );
    assert_eq!(
        call(&app, "GET", &path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/artists/missing/match-suggestions",
            Value::Null,
            token
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let group = items[0]["group"]["id"].as_str().unwrap().to_owned();
    let dismiss = format!("{path}/dismiss");
    let (status, after) = call(&app, "POST", &dismiss, json!({"group_id": group}), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (
            after["items"].as_array().unwrap().len(),
            &after["dismissed"]
        ),
        (0, &json!(1))
    );
    for (bad, expected) in [
        (json!({"group_id": "missing"}), StatusCode::NOT_FOUND),
        (
            json!({"group_id": group, "extra": 1}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        assert_eq!(call(&app, "POST", &dismiss, bad, token).await.0, expected);
    }

    let (_, listed) = call(&app, "GET", &path, Value::Null, token).await;
    assert_eq!(listed["dismissed_items"][0]["group"]["id"], group);
    let restore = format!("{path}/restore");
    assert_eq!(
        call(&app, "POST", &restore, json!({"group_id": group}), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for (bad, expected) in [
        (json!({"group_id": "missing"}), StatusCode::NOT_FOUND),
        (
            json!({"group_id": group, "extra": 1}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        assert_eq!(call(&app, "POST", &restore, bad, token).await.0, expected);
    }
    let (status, back) = call(&app, "POST", &restore, json!({"group_id": group}), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (
            back["items"].as_array().unwrap().len(),
            &back["dismissed"],
            back["dismissed_items"].as_array().unwrap().len()
        ),
        (1, &json!(0), 0)
    );
    let (_, again) = call(&app, "POST", &dismiss, json!({"group_id": group}), token).await;
    assert_eq!(again["dismissed"], 1);

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({"username": "outsider", "password": "owned matching outsider password"}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({"username": "outsider", "password": "owned matching outsider password"}),
        None,
    )
    .await;
    let outsider = session["token"].as_str();
    assert_eq!(
        call(&app, "GET", &path, Value::Null, outsider).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "POST", &dismiss, json!({"group_id": group}), outsider)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "POST", &restore, json!({"group_id": group}), outsider)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, own) = call(
        &app,
        "POST",
        "/api/v1/artist-matching",
        json!({"request_id": Uuid::new_v4()}),
        outsider,
    )
    .await;
    assert_eq!(
        own["matched"], 0,
        "Another account has no access to these recordings"
    );
}

async fn scanned() -> (tempfile::TempDir, axum::Router, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    tagged(&music.join("a.wav"), 1, "Song A", "Album A", Some(M1));
    tagged(&music.join("b.wav"), 2, "Song B", "Album B", Some(M1));
    tagged(&music.join("p.wav"), 3, "Song P", "Album A", None);
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        Some(&token),
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
    (dir, app, token, library)
}

async fn member(app: &axum::Router, admin: &str, library: &str) -> String {
    let (status, user) = call(
        app,
        "POST",
        "/api/v1/users",
        json!({"username": "member", "password": "owned matching member password"}),
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        app,
        "PUT",
        &format!(
            "/api/v1/libraries/{library}/members/{}",
            user["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(admin),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, session) = call(
        app,
        "POST",
        "/api/v1/login",
        json!({"username": "member", "password": "owned matching member password"}),
        None,
    )
    .await;
    session["token"].as_str().unwrap().to_owned()
}

async fn credit_and_group(app: &axum::Router, token: &str) -> (String, String) {
    let (_, credits) = call(app, "GET", "/api/v1/artists", Value::Null, Some(token)).await;
    let credit = credits["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artist| artist["name"] == "C4 Pedro")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, groups) = call(
        app,
        "GET",
        "/api/v1/artist-identity-groups",
        Value::Null,
        Some(token),
    )
    .await;
    (
        credit,
        groups["items"][0]["id"].as_str().unwrap().to_owned(),
    )
}

#[tokio::test]
async fn members_of_one_shared_library_match_and_dismiss_separately() {
    let (_dir, app, admin_token, library) = scanned().await;
    let member_token = member(&app, &admin_token, &library).await;
    for token in [&admin_token, &member_token] {
        let (status, receipt) = call(
            &app,
            "POST",
            "/api/v1/artist-matching",
            json!({"request_id": Uuid::new_v4()}),
            Some(token),
        )
        .await;
        assert_eq!((status, &receipt["matched"]), (StatusCode::OK, &json!(2)));
    }
    let (admin_credit, admin_group) = credit_and_group(&app, &admin_token).await;
    let (member_credit, member_group) = credit_and_group(&app, &member_token).await;
    assert_ne!(
        admin_group, member_group,
        "Each account owns its chosen artist"
    );

    let dismiss = format!("/api/v1/artists/{admin_credit}/match-suggestions/dismiss");
    let (status, after) = call(
        &app,
        "POST",
        &dismiss,
        json!({"group_id": admin_group}),
        Some(&admin_token),
    )
    .await;
    assert_eq!((status, &after["dismissed"]), (StatusCode::OK, &json!(1)));
    let (_, own) = call(
        &app,
        "GET",
        &format!("/api/v1/artists/{member_credit}/match-suggestions"),
        Value::Null,
        Some(&member_token),
    )
    .await;
    assert_eq!(own["dismissed"], 0, "A dismissal belongs to one account");
    assert_eq!(own["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        call(
            &app,
            "POST",
            &dismiss,
            json!({"group_id": member_group}),
            Some(&admin_token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "Another account's chosen artist cannot be dismissed"
    );
    let restore = format!("/api/v1/artists/{admin_credit}/match-suggestions/restore");
    assert_eq!(
        call(
            &app,
            "POST",
            &restore,
            json!({"group_id": member_group}),
            Some(&admin_token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "Another account's chosen artist cannot be restored"
    );
    let (_, member_view) = call(
        &app,
        "POST",
        &format!("/api/v1/artists/{member_credit}/match-suggestions/restore"),
        json!({"group_id": member_group}),
        Some(&member_token),
    )
    .await;
    assert_eq!(member_view["items"].as_array().unwrap().len(), 1);
    let (_, admin_view) = call(
        &app,
        "POST",
        &restore,
        json!({"group_id": admin_group}),
        Some(&admin_token),
    )
    .await;
    assert_eq!(admin_view["dismissed"], 0);
    assert_eq!(admin_view["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_locked_session_cannot_use_the_matching_routes() {
    let (_dir, app, token, _) = scanned().await;
    let (_, credits) = call(&app, "GET", "/api/v1/artists", Value::Null, Some(&token)).await;
    let credit = credits["items"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": "administrator test password", "pin": "1234", "idle_minutes": 0}),
            Some(&token),
        )
        .await
        .0,
        StatusCode::OK
    );
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
    for (method, path, body) in [
        (
            "POST",
            "/api/v1/artist-matching".to_owned(),
            json!({"request_id": Uuid::new_v4()}),
        ),
        (
            "GET",
            format!("/api/v1/artists/{credit}/match-suggestions"),
            Value::Null,
        ),
        (
            "POST",
            format!("/api/v1/artists/{credit}/match-suggestions/dismiss"),
            json!({"group_id": "any"}),
        ),
        (
            "POST",
            format!("/api/v1/artists/{credit}/match-suggestions/restore"),
            json!({"group_id": "any"}),
        ),
    ] {
        let (status, body) = call(&app, method, &path, body, Some(&token)).await;
        assert_eq!(status, StatusCode::LOCKED, "{path}");
        assert_eq!(body["code"], "locked", "{path}");
    }
}
