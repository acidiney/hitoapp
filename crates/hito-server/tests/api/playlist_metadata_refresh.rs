#[path = "playlist_metadata_fixture.rs"]
mod fixture;

use super::support::call;
use axum::http::StatusCode;
use fixture::{MetadataCopy, copy_identity, listening_state};
use hito_core::{ImportedPlaylistMetadataRefresh, RefreshImportedPlaylistMetadata};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn missing_albums_are_repaired_over_http_without_changing_the_imported_copy() {
    let f = MetadataCopy::start().await;
    let imported = f.get(&format!("/api/v1/playlists/{}", f.playlist)).await;
    assert_eq!(imported["playlist"]["kind"], "local");
    assert_eq!(imported["entries"].as_array().unwrap().len(), 4);
    assert_eq!(imported["entries"][0]["recording_id"], f.recording);
    assert_eq!(imported["entries"][3]["recording_id"], f.recording);
    assert!(imported["entries"][1]["recording_id"].is_null());
    assert!(imported["entries"][2]["recording_id"].is_null());
    assert_eq!(
        imported["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| Uuid::parse_str(entry["entry_id"].as_str().unwrap()).unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );
    let title_operation = Uuid::new_v4();
    let artist_operation = Uuid::new_v4();
    let like_operation = Uuid::new_v4();
    let progress_operation = Uuid::new_v4();
    let rename_operation = Uuid::new_v4();
    let (status, acknowledged) = call(
        &f.app,
        "POST",
        "/api/v1/sync",
        json!({"device_id": "http-metadata-copy", "operations": [
            {"id": title_operation, "action": {"kind": "recording_title_set", "recording_id": f.recording, "title": "Private retained title"}},
            {"id": artist_operation, "action": {"kind": "recording_artist_set", "recording_id": f.recording, "artist": "Private retained performer"}},
            {"id": like_operation, "action": {"kind": "like_set", "recording_id": f.recording, "liked": true}},
            {"id": progress_operation, "action": {"kind": "progress_set", "recording_id": f.recording, "position_ms": 4_200, "speed": 1.25}},
            {"id": rename_operation, "action": {"kind": "playlist_rename", "playlist_id": f.playlist, "name": "Private retained copy"}}
        ]}),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        acknowledged,
        json!({"acknowledged": [
            title_operation, artist_operation, like_operation, progress_operation, rename_operation
        ]})
    );
    let before = f.state().await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["albums"]["total"], 0);
    assert!(before["metadata"]["selected_observation"]["release_id"].is_null());
    assert_eq!(
        before["metadata"]["title"]["operation_id"],
        title_operation.to_string()
    );
    assert_eq!(
        before["metadata"]["artist"]["operation_id"],
        artist_operation.to_string()
    );
    assert_eq!(before["activity"]["history"].as_array().unwrap().len(), 1);
    assert_eq!(before["activity"]["history"][0]["position_ms"], 4_200);
    assert_eq!(before["activity"]["history"][0]["speed"], 1.25);
    assert_eq!(before["sources"], json!([]));
    let requests_before = f.provider_requests().await;
    assert_eq!(&requests_before[3..], &[0; 5]);

    f.publish_verified_album().await;
    let request = RefreshImportedPlaylistMetadata {
        request_id: Uuid::new_v4(),
        installation_revision: f.provider.revision,
    };
    let request_body = serde_json::to_value(&request).unwrap();
    let (status, receipt_bytes) = f.refresh(&request_body).await;
    assert_eq!(status, StatusCode::OK);
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(
        receipt,
        serde_json::to_value(ImportedPlaylistMetadataRefresh {
            request_id: request.request_id,
            playlist_id: Uuid::parse_str(&f.playlist).unwrap(),
            updated_recordings: 1,
            unchanged_recordings: 0,
            unavailable_recordings: 0,
        })
        .unwrap()
    );

    let after = f.state().await;
    assert_eq!(
        copy_identity(&after["copy"]),
        copy_identity(&before["copy"])
    );
    assert_eq!(after["catalog"]["total"], 1);
    let recording = &after["catalog"]["items"][0];
    assert_eq!(recording["id"], f.recording);
    assert_eq!(recording["title"], "Private retained title");
    assert_eq!(recording["artist"], "Private retained performer");
    assert_eq!(recording["liked"], true);
    assert_eq!(recording["available"], false);
    for field in ["title", "artist"] {
        for retained in ["personal", "operation_id", "value"] {
            assert_eq!(
                after["metadata"][field][retained],
                before["metadata"][field][retained]
            );
        }
    }
    assert_eq!(
        after["metadata"]["title"]["inherited"],
        "Verified recording"
    );
    assert_eq!(
        after["metadata"]["artist"]["inherited"],
        "Verified performer"
    );
    assert_eq!(
        after["metadata"]["observations"]["items"]
            .as_array()
            .unwrap()
            .len(),
        before["metadata"]["observations"]["items"]
            .as_array()
            .unwrap()
            .len()
            + 1
    );
    for observation in before["metadata"]["observations"]["items"]
        .as_array()
        .unwrap()
    {
        assert!(
            after["metadata"]["observations"]["items"]
                .as_array()
                .unwrap()
                .contains(observation)
        );
    }
    assert_eq!(
        listening_state(&after["activity"]),
        listening_state(&before["activity"])
    );
    assert_eq!(after["sources"], before["sources"]);
    assert_eq!(after["source_choice"], before["source_choice"]);
    assert_eq!(after["albums"]["total"], 1);
    let album = &after["albums"]["items"][0];
    assert_eq!(album["title"], "Verified provider album");
    assert_eq!(album["track_count"], 1);
    assert!(album["artist"].is_null() && album["artist_id"].is_null());
    let album_path = format!("/api/v1/releases/{}", album["id"].as_str().unwrap());
    let album_detail = f.get(&album_path).await;
    assert_eq!(album_detail["tracks"]["total"], 1);
    let member = &album_detail["tracks"]["items"][0];
    assert_eq!(member["id"], f.recording);
    assert_eq!(member["liked"], true);
    assert_eq!(member["disc_number"], 1);
    assert_eq!(member["track_number"], 4);
    assert_eq!(
        after["metadata"]["selected_observation"]["release_id"],
        album["id"]
    );

    let previous_events = before["events"]["operations"].as_array().unwrap();
    let events = after["events"]["operations"].as_array().unwrap();
    assert_eq!(&events[..previous_events.len()], previous_events);
    assert_eq!(events.len(), previous_events.len() + 1);
    let publication = &events.last().unwrap()["publication"];
    assert_eq!(publication["kind"], "playlist_metadata_refreshed");
    assert_eq!(publication["playlist_id"], f.playlist);
    assert!(publication.get("entries").is_none() && publication.get("name").is_none());
    assert_eq!(publication["recordings"].as_array().unwrap().len(), 1);
    assert_eq!(publication["recordings"][0]["recording_id"], f.recording);
    let mut expected_requests = requests_before;
    expected_requests[1] += 2;
    expected_requests[2] += 1;
    assert_eq!(f.provider_requests().await, expected_requests);
    assert_eq!(
        call(
            &f.app,
            "GET",
            &format!("/api/v1/recordings/{}/download", f.recording),
            Value::Null,
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    f.provider.server.offline().await;
    let (status, replay_bytes) = f.refresh(&request_body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay_bytes, receipt_bytes);
    let (status, import_replay) = call(
        &f.app,
        "POST",
        &f.provider.import_path,
        f.import_body.clone(),
        Some(&f.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(import_replay, f.import_receipt);
    assert_eq!(f.state().await, after);
    assert_eq!(f.get(&album_path).await, album_detail);
    assert_eq!(f.provider_requests().await, expected_requests);
    assert_eq!(
        call(
            &f.app,
            "GET",
            &format!("/api/v1/recordings/{}/download", f.recording),
            Value::Null,
            Some(&f.token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
