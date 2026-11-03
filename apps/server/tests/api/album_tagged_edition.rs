use crate::support::{admin, call};
use axum::http::StatusCode;
use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    support::write_wav,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use uuid::Uuid;

const ARTIST: &str = "11111111-1111-4111-8111-111111111111";
const RELEASE: &str = "0f700000-0000-4000-8000-0000000000a1";
const GROUP: &str = "0f700000-0000-4000-8000-0000000000b1";
const RECORDING: &str = "0f700000-0000-4000-8000-0000000000c1";

fn flac(directory: &std::path::Path, music: &std::path::Path) {
    let source = directory.join("tagged.wav");
    write_wav(&source, 1);
    let path = music.join("tagged.flac");
    let status = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(&source)
        .args(["-c:a", "flac"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let mut tag = Tag::new(TagType::VorbisComments);
    tag.set_title("Tagged song".into());
    tag.set_artist("Performer".into());
    tag.set_album("Tagged Album".into());
    tag.insert_text(ItemKey::MusicBrainzRecordingId, RECORDING.into());
    tag.insert_text(ItemKey::MusicBrainzReleaseId, RELEASE.into());
    tag.save_to_path(&path, Default::default()).unwrap();
}

#[tokio::test]
async fn a_tagged_album_reports_its_edition_and_the_details_pages_show_the_tagged_ids() {
    let musicbrainz = FakeMusicBrainz::start().await;
    musicbrainz.recording(
        RECORDING,
        Reply::Recordings(vec![(RECORDING.into(), vec![ARTIST.into()])]),
    );
    musicbrainz.release(RELEASE, Reply::ReleaseGroup(RELEASE.into(), GROUP.into()));
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    flac(dir.path(), &music);
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let token = Some(token.as_str());
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, token).await;
    let library = libraries[0]["id"].as_str().unwrap();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        token,
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
    let (_, albums) = call(&app, "GET", "/api/v1/releases", Value::Null, token).await;
    let album = albums["items"][0]["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/albums/{album}/tagged-edition");

    assert_eq!(
        call(&app, "POST", &path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/albums/missing/tagged-edition",
            Value::Null,
            token
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, unmatched) = call(&app, "POST", &path, Value::Null, token).await;
    assert_eq!(
        (status, &unmatched),
        (StatusCode::OK, &json!({"status": "none"}))
    );

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/artist-matching/provider-evidence",
        json!({"request_id": Uuid::new_v4()}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, found) = call(&app, "POST", &path, Value::Null, token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["status"], "found");
    assert_eq!(found["edition"]["edition_id"], RELEASE);
    assert_eq!(found["edition"]["release_group_id"], GROUP);

    let (_, release) = call(
        &app,
        "GET",
        &format!("/api/v1/releases/{album}/metadata"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(release["musicbrainz"]["release_id"], RELEASE);
    assert_eq!(release["musicbrainz"]["release_group_id"], Value::Null);
    let recording = found["edition"]["recording_id"].as_str().unwrap();
    let (_, details) = call(
        &app,
        "GET",
        &format!("/api/v1/recordings/{recording}/metadata"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(details["musicbrainz_recording_id"], RECORDING);

    let bob = json!({"username": "bob", "password": "tagged bob password"});
    let (status, _) = call(&app, "POST", "/api/v1/users", bob.clone(), token).await;
    assert!(status.is_success(), "{status}");
    let (_, session) = call(&app, "POST", "/api/v1/login", bob, None).await;
    let bob_token = session["token"].as_str().unwrap().to_owned();
    let requests = musicbrainz.requests().len();
    let (status, _) = call(&app, "POST", &path, Value::Null, Some(&bob_token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(musicbrainz.requests().len(), requests);
    let (status, again) = call(&app, "POST", &path, Value::Null, token).await;
    assert_eq!(
        (status, again["status"].as_str()),
        (StatusCode::OK, Some("found"))
    );
}
