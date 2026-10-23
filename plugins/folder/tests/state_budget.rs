#![cfg(unix)]
#[path = "support/binary.rs"]
mod binary;
#[allow(dead_code)]
mod support;

use hito_plugin_sdk::{ProviderRecording, SearchPage, SourcePage};
use sqlx::{ConnectOptions, Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{fs, path::Path};

const STATE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    recording: serde_json::Value,
    source: String,
}

fn snapshot(service: &binary::Service, title: &str, bytes: &[u8]) -> Snapshot {
    let page: SearchPage = service.post(
        "/v1/search",
        serde_json::json!({"query":"recording","limit":50}),
    );
    page.validate(50).unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.next_cursor.is_none());
    let recording = &page.items[0];
    assert_eq!(recording.title, title);
    let detail: ProviderRecording = service.post(
        "/v1/recordings/detail",
        serde_json::json!({"recording_id":recording.id}),
    );
    detail.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&detail).unwrap(),
        serde_json::to_value(recording).unwrap()
    );
    let sources: SourcePage = service.post(
        "/v1/sources",
        serde_json::json!({"recording_id":recording.id}),
    );
    sources.validate().unwrap();
    assert_eq!(sources.items.len(), 1);
    assert!(sources.items[0].ready);
    let source = &sources.items[0].id;
    let tag = bytes
        .windows(title.len())
        .position(|part| part == title.as_bytes())
        .unwrap();
    assert_eq!(&bytes[36..40], b"data");
    for range in [tag..tag + title.len(), 44..76] {
        assert_eq!(
            service.audio(source, range.start, range.end),
            (206, bytes[range].to_vec())
        );
    }
    Snapshot {
        recording: serde_json::to_value(recording).unwrap(),
        source: source.clone(),
    }
}

async fn fill_owned_state(state: &Path) {
    let database = state.join("identity.sqlite");
    let options = SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(false)
        .disable_statement_logging();
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("PRAGMA page_size = 65536")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("VACUUM")
        .execute(&mut connection)
        .await
        .unwrap();
    let page_size: i64 = sqlx::query_scalar("PRAGMA page_size")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(page_size, 65536);
    let pages: i64 = sqlx::query_scalar("PRAGMA max_page_count = 1024")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(pages, 1024);
    sqlx::query("CREATE TABLE fixture_padding (payload BLOB NOT NULL)")
        .execute(&mut connection)
        .await
        .unwrap();
    let mut full = false;
    for _ in 0..1024 {
        match sqlx::query("INSERT INTO fixture_padding VALUES (zeroblob(60000))")
            .execute(&mut connection)
            .await
        {
            Ok(_) => {}
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("13") => {
                full = true;
                break;
            }
            Err(error) => panic!("owned fixture padding failed: {error}"),
        }
    }
    connection.close().await.unwrap();
    assert!(full, "owned fixture did not reach its bounded page budget");
    assert_eq!(fs::metadata(database).unwrap().len(), STATE_BYTES);
}

#[tokio::test]
async fn a_large_page_state_budget_rejects_growth_without_losing_committed_identity() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("private-root");
    fs::create_dir(&music).unwrap();
    let title = "Retained budget recording";
    let seed = support::write_wav(&music.join("private-seed.wav"), Some(title), 440);
    let state = directory.path().join("private-state");
    binary::initialize(&music, &state);
    let service = binary::Service::start(&music, &state, &directory.path().join("seed.jsonl"));
    let initial = snapshot(&service, title, &seed);
    service.stop();

    fill_owned_state(&state).await;
    let service = binary::Service::start(&music, &state, &directory.path().join("padded.jsonl"));
    assert_eq!(snapshot(&service, title, &seed), initial);
    service.stop();

    let artist = "a".repeat(2048);
    let album = "b".repeat(2048);
    let mut added = Vec::new();
    for index in 0..48 {
        let prefix = format!("Added budget recording {index:02} ");
        let title = format!("{prefix}{}", "t".repeat(2048 - prefix.len()));
        let path = music.join(format!("private-added-{index:02}.wav"));
        let bytes = support::write_tagged_wav(&path, Some(&title), 500 + index, &artist, &album);
        assert_eq!(&bytes[36..40], b"data");
        assert_ne!(bytes[44..76], seed[44..76]);
        added.push(path);
    }
    binary::Service::reject_start(&music, &state, &directory.path().join("exhausted.jsonl"));
    for path in added {
        fs::remove_file(path).unwrap();
    }

    let service = binary::Service::start(&music, &state, &directory.path().join("recovered.jsonl"));
    assert_eq!(snapshot(&service, title, &seed), initial);
    service.stop();
}
