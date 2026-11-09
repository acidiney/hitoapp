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
