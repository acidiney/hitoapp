use hito_plugin_sdk::{
    Manifest, PlaylistPage, ProviderPlaylist, ProviderRecording, RecordingMetadata,
};
use serde::{Serialize, de::DeserializeOwned};

fn unchanged<T: DeserializeOwned + Serialize>(wire: &[u8]) {
    let decoded: T = serde_json::from_slice(wire).expect("the owned legacy wire must deserialize");
    assert_eq!(
        serde_json::to_vec(&decoded).expect("the owned legacy wire must serialize"),
        wire
    );
}

#[test]
fn catalog_contracts_do_not_change_known_manifest_or_recording_wire_bytes() {
    unchanged::<Manifest>(br#"{"id":"org.hito.catalog-fixture","name":"Owned catalog fixture","protocol_version":1,"capabilities":["search","catalog","metadata","sources","playlists","artwork"],"content_policy":{"source_persistence":"explicit_download"}}"#);
    unchanged::<ProviderRecording>(br#"{"id":"recording:owned","title":"Owned recording","artist":"Owned performer","duration_ms":123000,"release":{"id":"release:owned","title":"Owned album","artist":"Owned album credit"},"disc_number":1,"track_number":2,"revision":"owned-revision"}"#);
    unchanged::<RecordingMetadata>(br#"{"recording":{"id":"recording:owned","title":"Owned recording","artist":"Owned performer","duration_ms":123000,"release":null,"disc_number":null,"track_number":null,"revision":null},"musical_title":"Owned musical title","album_title":"Owned album","source_ids":["source:owned"]}"#);
}

#[test]
fn catalog_contracts_do_not_change_known_playlist_wire_bytes() {
    unchanged::<ProviderPlaylist>(br#"{"id":"playlist:owned","name":"Owned playlist","snapshot":"owned-snapshot","total_count":1,"attribution":{"label":"Owned attribution"}}"#);
    unchanged::<PlaylistPage>(br#"{"playlist_id":"playlist:owned","snapshot":"owned-snapshot","items":[{"occurrence_id":"entry:owned","item":{"kind":"recording","recording":{"id":"recording:owned","title":"Owned recording","artist":"Owned performer","duration_ms":123000,"release":null,"disc_number":null,"track_number":null,"revision":null}}}],"next_cursor":null}"#);
}
