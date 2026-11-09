use super::recording_groups::import;
use super::support::{admin, app, call};
use axum::http::StatusCode;
use serde_json::{Value, json};

fn imported(id: &str, artist: &str, artists: Value) -> Value {
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": id, "title": format!("Song {id}"), "artist": artist, "duration_ms": 1000,
        "release": null, "disc_number": null, "track_number": null, "revision": "v1",
        "artists": artists
    }}})
}

#[tokio::test]
async fn structured_credits_group_reordered_collaborations_and_are_read_back_in_order() {
    let (_dir, app) = app().await;
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let (origin, recordings) = import(
        &app,
        &session,
        vec![
            imported(
                "one",
                "Ada & Bo",
                json!([{"name": "Ada", "join": " & "}, {"name": "Bo"}]),
            ),
            imported(
                "two",
                "Bo feat. Ada",
                json!([{"name": "Bo", "join": " feat. "}, {"name": "Ada", "role": "featured"}]),
            ),
            imported(
                "three",
                "Ada, Bo",
                json!([{"name": "Bo"}, {"name": "Ada", "join": null}]),
            ),
        ],
    )
    .await;
    let (status, artists) = call(&app, "GET", "/api/v1/artists", Value::Null, token).await;
    assert_eq!(status, StatusCode::OK);
    let credits: Vec<(&str, i64)> = artists["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artist| {
            (
                artist["name"].as_str().unwrap(),
                artist["recording_count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(credits, [("Ada", 2), ("Ada & Bo", 2), ("Bo", 3)]);
    let id = |name: &str| {
        artists["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artist| artist["name"] == name)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let titles = |page: &Value| -> Vec<String> {
        let mut titles: Vec<String> = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|recording| recording["title"].as_str().unwrap().to_owned())
            .collect();
        titles.sort();
        titles
    };
    let (status, ada_page) = call(
        &app,
        "GET",
        &format!("/api/v1/artists/{}", id("Ada")),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(titles(&ada_page["recordings"]), ["Song one", "Song three"]);
    assert_eq!(ada_page["featured_recordings"]["total"], 1);
    assert_eq!(titles(&ada_page["featured_recordings"]), ["Song two"]);
    assert_eq!(ada_page["artist"]["featured_recording_count"], 1);
    let (_, bo_page) = call(
        &app,
        "GET",
        &format!("/api/v1/artists/{}", id("Bo")),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(
        titles(&bo_page["recordings"]),
        ["Song one", "Song three", "Song two"]
    );
    assert_eq!(bo_page["featured_recordings"]["total"], 0);
    let (_, listing) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings?artist={}", id("Ada")),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(titles(&listing), ["Song one", "Song three"]);

    let (status, detail) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{}/metadata", recordings[1]),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let bo = id("Bo");
    let ada = id("Ada");
    assert_eq!(detail["artist"]["value"], "Bo feat. Ada");
    assert_eq!(
        detail["credited_artists"],
        json!([
            {"name": "Bo", "role": "main", "join": " feat. ", "artist_id": bo},
            {"name": "Ada", "role": "featured", "join": null, "artist_id": ada}
        ])
    );
    let (_, old) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{}/metadata", recordings[0]),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(old["credited_artists"][1]["name"], "Bo");
    assert_eq!(old["credited_artists"][1]["artist_id"], bo);
    assert_eq!(old["credited_artists"][0]["artist_id"], ada);
    origin.shutdown().await;
}

#[tokio::test]
async fn featured_appearances_page_by_offset_and_stay_private_to_the_account() {
    let (_dir, app) = app().await;
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let featuring = |id: &str| {
        imported(
            id,
            "Ada feat. Cy",
            json!([{"name": "Ada", "join": " feat. "}, {"name": "Cy", "role": "featured"}]),
        )
    };
    let (origin, _) = import(
        &app,
        &session,
        vec![
            featuring("c"),
            featuring("a"),
            featuring("d"),
            featuring("b"),
        ],
    )
    .await;
    let (_, artists) = call(&app, "GET", "/api/v1/artists", Value::Null, token).await;
    let cy = artists["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artist| artist["name"] == "Cy")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let titles = |page: &Value| -> Vec<String> {
        page["featured_recordings"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|recording| recording["title"].as_str().unwrap().to_owned())
            .collect()
    };
    let mut continued = Vec::new();
    for offset in [0, 3, 6] {
        let (status, page) = call(
            &app,
            "GET",
            &format!("/api/v1/artists/{cy}?featured_offset={offset}&limit=3"),
            Value::Null,
            token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["featured_recordings"]["total"], 4);
        assert_eq!(page["featured_recordings"]["offset"], offset);
        continued.extend(titles(&page));
    }
    assert_eq!(continued, ["Song a", "Song b", "Song c", "Song d"]);

    let credentials = json!({"username": "stranger", "password": "stranger paging password"});
    assert_eq!(
        call(&app, "POST", "/api/v1/users", credentials.clone(), token)
            .await
            .0,
        StatusCode::CREATED
    );
    let (_, other) = call(&app, "POST", "/api/v1/login", credentials, None).await;
    let other_token = other["token"].as_str().unwrap();
    let (status, _) = call(
        &app,
        "GET",
        &format!("/api/v1/artists/{cy}?featured_offset=0&limit=3"),
        Value::Null,
        Some(other_token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    origin.shutdown().await;
}

#[tokio::test]
async fn collaboration_rows_carry_their_credited_artists_in_order_and_solo_rows_do_not() {
    let (_dir, app) = app().await;
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let (origin, recordings) = import(
        &app,
        &session,
        vec![
            imported(
                "one",
                "Ada & Bo",
                json!([{"name": "Ada", "join": " & "}, {"name": "Bo"}]),
            ),
            imported(
                "two",
                "Bo feat. Ada",
                json!([{"name": "Bo", "join": " feat. "}, {"name": "Ada", "role": "featured"}]),
            ),
            imported("three", "Solo Act", json!([])),
        ],
    )
    .await;
    let (_, artists) = call(&app, "GET", "/api/v1/artists", Value::Null, token).await;
    let id = |name: &str| {
        artists["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artist| artist["name"] == name)
            .unwrap()["id"]
            .clone()
    };
    let (status, listing) = call(&app, "GET", "/api/v1/recordings", Value::Null, token).await;
    assert_eq!(status, StatusCode::OK);
    let row = |title: &str| -> Value {
        listing["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|recording| recording["title"] == title)
            .unwrap()
            .clone()
    };
    assert_eq!(
        row("Song one")["credited_artists"],
        json!([
            {"id": id("Ada"), "name": "Ada", "role": "main", "join": " & "},
            {"id": id("Bo"), "name": "Bo", "role": "main"}
        ])
    );
    assert_eq!(
        row("Song two")["credited_artists"],
        json!([
            {"id": id("Bo"), "name": "Bo", "role": "main", "join": " feat. "},
            {"id": id("Ada"), "name": "Ada", "role": "featured"}
        ])
    );
    assert!(row("Song three").get("credited_artists").is_none());

    let playlist = uuid::Uuid::new_v4();
    let operations = json!({"device_id": "desktop-a", "operations": [
        {"id": uuid::Uuid::new_v4(), "action": {"kind": "playlist_create", "playlist_id": playlist, "name": "Mixed"}},
        {"id": uuid::Uuid::new_v4(), "action": {"kind": "entry_insert", "playlist_id": playlist, "entry_id": uuid::Uuid::new_v4(), "recording_id": recordings[0], "position": 0}}
    ]});
    assert_eq!(
        call(&app, "POST", "/api/v1/sync", operations, token)
            .await
            .0,
        StatusCode::OK
    );
    let (_, detail) = call(
        &app,
        "GET",
        &format!("/api/v1/playlists/{playlist}"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(detail["entries"][0]["credited_artists"][1]["name"], "Bo");
    origin.shutdown().await;
}
