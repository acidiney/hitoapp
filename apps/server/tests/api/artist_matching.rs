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
