use super::support::{admin, call};
use axum::http::StatusCode;
use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    playlist_service as provider,
};
use serde_json::{Value, json};
use uuid::Uuid;

const ADA: &str = "a0000000-0000-4000-8000-00000000000a";
const BO: &str = "b0000000-0000-4000-8000-00000000000b";
const SONGS: [(&str, &str, &str, u64); 6] = [
    ("ada-1", "Ada", "Ada Morning", 201_000),
    ("ada-2", "Ada", "Ada Evening", 187_500),
    ("bo-1", "Bo", "Bo Harbor", 243_000),
    ("bo-2", "Bo", "Bo Rooms", 150_000),
    ("duet-1", "Ada & Bo", "Duet Lights", 211_000),
    ("duet-2", "Ada & Bo", "Duet Wind", 176_000),
];

fn identify(musicbrainz: &FakeMusicBrainz, name: &str, id: &str) {
    musicbrainz.search(name, Reply::Candidates(vec![(id.into(), name.into())]));
    musicbrainz.recordings_of(
        id,
        Reply::Discography(
            SONGS
                .iter()
                .filter(|song| song.1 == name)
                .map(|song| (song.2.to_owned(), Some(song.3)))
                .collect(),
        ),
    );
}

#[tokio::test]
async fn a_combined_text_credit_gets_its_members_identified_through_the_api() {
    let musicbrainz = FakeMusicBrainz::start().await;
    identify(&musicbrainz, "Ada", ADA);
    identify(&musicbrainz, "Bo", BO);
    musicbrainz.search("Ada \\& Bo", Reply::Candidates(vec![]));
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let app = hito_server::router(hito_core::open(config).await.unwrap());
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let origin = provider::ProviderServer::start().await;
    origin
        .reply(
            "v1/manifest",
            json!({"id": "org.hito.member-fixture", "name": "Member fixture",
                "protocol_version": 1, "capabilities": ["playlists"]}),
        )
        .await;
    provider::playlist(
        &origin,
        "member-list",
        "member-v1",
        SONGS
            .iter()
            .map(|(id, artist, title, length)| {
                json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
                    "id": id, "title": title, "artist": artist, "duration_ms": length,
                    "release": null, "disc_number": null, "track_number": null, "revision": "v1"
                }}})
            })
            .collect(),
    )
    .await;
    let (_, service) = call(
        &app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "Member fixture", "base_url": origin.url}),
        token,
    )
    .await;
    let (_, installation) = call(
        &app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        token,
    )
    .await;
    let (_, receipt) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/member-list"
        }),
        token,
    )
    .await;
    let (_, detail) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/playlists/{}",
            receipt["playlist_id"].as_str().unwrap()
        ),
        Value::Null,
        token,
    )
    .await;
    let duet = detail["entries"][4]["recording_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut after = Value::Null;
    let mut cursors = Vec::new();
    for _ in 0..32 {
        let (status, walked) = call(
            &app,
            "POST",
            "/api/v1/artist-matching/provider-evidence",
            json!({"request_id": Uuid::new_v4(), "after": after}),
            token,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{walked}");
        if walked["next_after"].is_null() {
            break;
        }
        after = walked["next_after"].clone();
        cursors.push(after.as_str().unwrap().to_owned());
    }
    assert!(
        cursors.iter().any(|cursor| cursor.starts_with("member:")),
        "{cursors:?}"
    );
    let (_, metadata) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{duet}/metadata"),
        Value::Null,
        token,
    )
    .await;
    let members = metadata["credited_artists"].as_array().unwrap();
    let names: Vec<&str> = members
        .iter()
        .map(|member| member["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Ada", "Bo"]);
    let (_, associations) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{duet}/artist-associations"),
        Value::Null,
        token,
    )
    .await;
    for (member, expected) in members.iter().zip([ADA, BO]) {
        let (status, choice) = call(
            &app,
            "GET",
            &format!(
                "/api/v1/recordings/{duet}/credit-members/{}/artist-identity",
                member["artist_id"].as_str().unwrap()
            ),
            Value::Null,
            token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(choice["state"]["status"], "selected", "{choice}");
        let association = choice["state"]["association_id"].clone();
        let linked = associations
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["id"] == association)
            .unwrap_or_else(|| panic!("{choice} {associations}"));
        assert_eq!(linked["provider_artist_id"], expected);
        assert_eq!(linked["evidence"], "matched_member_solo_credit");
    }
    let (_, queue) = call(
        &app,
        "GET",
        "/api/v1/artist-matching/review-queue",
        Value::Null,
        token,
    )
    .await;
    assert!(
        queue["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["name"] != "Ada & Bo"),
        "{queue}"
    );
}
