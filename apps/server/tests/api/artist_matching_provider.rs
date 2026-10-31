use super::support::{admin, call};
use axum::http::StatusCode;
use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    playlist_service as provider,
    support::write_wav,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use uuid::Uuid;

const ARTIST: &str = "11111111-1111-4111-8111-111111111111";
const ISRC: &str = "USRC17607839";

async fn matched_pair() -> (
    tempfile::TempDir,
    axum::Router,
    String,
    FakeMusicBrainz,
    provider::ProviderServer,
    String,
) {
    let musicbrainz = FakeMusicBrainz::start().await;
    musicbrainz.isrc(
        ISRC,
        Reply::Recordings(vec![(
            "0f700000-0000-4000-8000-0000000000a1".into(),
            vec![ARTIST.into()],
        )]),
    );
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let path = music.join("a.wav");
    write_wav(&path, 1);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Local song".into());
    tag.set_artist("C4 Pedro".into());
    tag.insert_text(ItemKey::MusicBrainzArtistId, ARTIST.into());
    tag.save_to_path(&path, Default::default()).unwrap();
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
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

    let entry = json!({"occurrence_id": "only", "item": {"kind": "recording", "recording": {
        "id": "imported", "title": "Imported song", "artist": "C4 Pedro", "duration_ms": 1000,
        "release": null, "disc_number": null, "track_number": null, "revision": "v1",
        "isrc": "us-rc1-76-07839"
    }}});
    let (origin, imported) = import(&app, &token, vec![entry]).await;
    (dir, app, token, musicbrainz, origin, imported[0].clone())
}

/// Imports one playlist of recordings through an owned provider and returns their identifiers.
async fn import(
    app: &axum::Router,
    token: &str,
    entries: Vec<Value>,
) -> (provider::ProviderServer, Vec<String>) {
    let token = Some(token);
    let origin = provider::ProviderServer::start().await;
    origin
        .reply(
            "v1/manifest",
            json!({"id": "org.hito.isrc-fixture", "name": "ISRC fixture",
                "protocol_version": 1, "capabilities": ["playlists"]}),
        )
        .await;
    provider::playlist(&origin, "isrc-list", "isrc-v1", entries).await;
    let (_, service) = call(
        app,
        "POST",
        "/api/v1/plugins/services",
        json!({"name": "ISRC fixture", "base_url": origin.url}),
        token,
    )
    .await;
    let (_, installation) = call(
        app,
        "POST",
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        token,
    )
    .await;
    let (status, receipt) = call(
        app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/isrc-list"
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, detail) = call(
        app,
        "GET",
        &format!(
            "/api/v1/playlists/{}",
            receipt["playlist_id"].as_str().unwrap()
        ),
        Value::Null,
        token,
    )
    .await;
    let recordings = detail["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["recording_id"].as_str().unwrap().to_owned())
        .collect();
    (origin, recordings)
}

#[tokio::test]
async fn an_imported_track_with_an_isrc_joins_the_tagged_local_artist_through_the_api() {
    let (_dir, app, token, musicbrainz, _origin, imported) = matched_pair().await;
    let token = Some(token.as_str());
    let (status, tags) = call(
        &app,
        "POST",
        "/api/v1/artist-matching",
        json!({"request_id": Uuid::new_v4()}),
        token,
    )
    .await;
    assert_eq!((status, &tags["matched"]), (StatusCode::OK, &json!(1)));

    let body = json!({"request_id": Uuid::new_v4()});
    let path = "/api/v1/artist-matching/provider-evidence";
    let (status, receipt) = call(&app, "POST", path, body.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["request_id"], body["request_id"]);
    assert_eq!(
        (
            &receipt["matched"],
            &receipt["uncertain"],
            &receipt["declined"],
            &receipt["unavailable"],
            &receipt["next_after"]
        ),
        (&json!(1), &json!(0), &json!(0), &json!(0), &json!("name:"))
    );
    let lookups = musicbrainz.requests().len();
    assert_eq!(
        call(&app, "POST", path, body.clone(), token).await,
        (StatusCode::OK, receipt),
        "An exact retry replays the receipt"
    );
    assert_eq!(musicbrainz.requests().len(), lookups);
    let mut changed = body.clone();
    changed["limit"] = json!(1);
    assert_eq!(
        call(&app, "POST", path, changed, token).await.0,
        StatusCode::CONFLICT
    );

    let (_, groups) = call(
        &app,
        "GET",
        "/api/v1/artist-identity-groups",
        Value::Null,
        token,
    )
    .await;
    assert_eq!(groups["items"].as_array().unwrap().len(), 1);
    assert_eq!(groups["items"][0]["provider_artist_id"], ARTIST);
    assert_eq!(
        groups["items"][0]["recording_count"], 2,
        "The imported and the local track are one artist without a click"
    );
    let (_, associations) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{imported}/artist-associations"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(associations[0]["evidence"], "matched_isrc");
    assert_eq!(associations[0]["provider_artist_id"], ARTIST);
}

#[tokio::test]
async fn provider_evidence_matching_validates_input_access_and_locks() {
    let (_dir, app, token, _musicbrainz, _origin, _) = matched_pair().await;
    let token = Some(token.as_str());
    let path = "/api/v1/artist-matching/provider-evidence";
    for bad in [
        json!({"request_id": Uuid::new_v4(), "limit": 0}),
        json!({"request_id": Uuid::new_v4(), "limit": 101}),
        json!({"request_id": Uuid::new_v4(), "extra": 1}),
        json!({"request_id": Uuid::nil()}),
    ] {
        assert_eq!(
            call(&app, "POST", path, bad, token).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            json!({"request_id": Uuid::new_v4()}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/pin",
            json!({"password": "administrator test password", "pin": "1234", "idle_minutes": 0}),
            token,
        )
        .await
        .0,
        StatusCode::OK
    );
    call(&app, "POST", "/api/v1/account/lock", Value::Null, token).await;
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            json!({"request_id": Uuid::new_v4()}),
            token
        )
        .await
        .0,
        StatusCode::LOCKED
    );
}

#[tokio::test]
async fn a_credit_is_matched_by_name_only_when_the_artists_recordings_corroborate_it_through_the_api()
 {
    let musicbrainz = FakeMusicBrainz::start().await;
    musicbrainz.search(
        "Nova Quartet",
        Reply::Candidates(vec![(ARTIST.into(), "Nova Quartet".into())]),
    );
    musicbrainz.recordings_of(
        ARTIST,
        Reply::Discography(vec![
            ("First Song".into(), Some(100_500)),
            ("second song".into(), Some(120_000)),
        ]),
    );
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
    let app = hito_server::router(hito);
    let session = admin(&app).await;
    let token = Some(session.as_str());
    let entry = |id: &str, title: &str, length: u64| {
        json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
            "id": id, "title": title, "artist": "Nova Quartet", "duration_ms": length,
            "release": null, "disc_number": null, "track_number": null, "revision": "v1"
        }}})
    };
    let (_origin, recordings) = import(
        &app,
        &session,
        vec![
            entry("one", "First Song", 100_000),
            entry("two", "Second Song", 120_500),
        ],
    )
    .await;

    let path = "/api/v1/artist-matching/provider-evidence";
    let (status, evidence) = call(
        &app,
        "POST",
        path,
        json!({"request_id": Uuid::new_v4()}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(evidence["matched"], 0);
    assert_eq!(evidence["next_after"], "name:");
    assert!(
        musicbrainz.requests().is_empty(),
        "The evidence phase sends no name"
    );

    let body = json!({"request_id": Uuid::new_v4(), "after": evidence["next_after"]});
    let (status, names) = call(&app, "POST", path, body.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&names["matched"], &names["uncertain"], &names["next_after"]),
        (&json!(2), &json!(0), &Value::Null)
    );
    let asked = musicbrainz.requests().len();
    assert_eq!(asked, 2);
    assert_eq!(
        call(&app, "POST", path, body, token).await,
        (StatusCode::OK, names),
        "An exact retry replays the receipt"
    );
    assert_eq!(musicbrainz.requests().len(), asked);
    let (_, associations) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{}/artist-associations", recordings[0]),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(associations[0]["evidence"], "matched_name_discography");
    assert_eq!(associations[0]["provider_artist_id"], ARTIST);
    let long = "x".repeat(65);
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            json!({"request_id": Uuid::new_v4(), "after": long}),
            token
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
