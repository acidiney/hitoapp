use crate::fixture::Library;
use serde_json::json;

pub async fn publish(f: &Library, conflicting: bool) {
    let copy = f
        .hito
        .collection
        .playlist(&f.actor, &f.first.playlist_id.to_string())
        .await
        .unwrap();
    let origin = copy.origin.as_ref().unwrap();
    let first = copy.entries[0]
        .origin
        .as_ref()
        .unwrap()
        .provider_recording_id
        .as_ref()
        .unwrap();
    let second = copy.entries[1]
        .origin
        .as_ref()
        .unwrap()
        .provider_recording_id
        .as_ref()
        .unwrap();
    assert_ne!(first, second);
    let snapshot = if conflicting {
        "conflicting-releases"
    } else {
        "consistent-releases"
    };
    let release = json!({"id": "release/metadata-atomic", "title": "Verified shared album"});
    let other_release = if conflicting {
        json!({"id": "release/metadata-atomic", "title": "Conflicting album metadata"})
    } else {
        release.clone()
    };
    f.origin.reply("v1/playlists/detail", json!({
        "id": origin.provider_playlist_id, "name": "Current verified metadata",
        "snapshot": snapshot, "total_count": 2, "attribution": {"label": "Owned metadata fixture"}
    })).await;
    f.origin.reply("v1/playlists/items", json!({
        "playlist_id": origin.provider_playlist_id, "snapshot": snapshot,
        "items": [
            {"item": {"kind": "recording", "recording": {
                "id": first, "title": "Verified first recording", "artist": "Verified first performer",
                "duration_ms": 91_000, "release": release, "disc_number": 1, "track_number": 1,
                "revision": snapshot
            }}},
            {"item": {"kind": "recording", "recording": {
                "id": second, "title": "Verified second recording", "artist": "Verified second performer",
                "duration_ms": 121_000, "release": other_release, "disc_number": 1, "track_number": 2,
                "revision": snapshot
            }}}
        ], "next_cursor": null
    })).await;
}
