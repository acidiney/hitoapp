use hito_core::test_support::album_edition_service as album;
use hito_plugin_sdk::{
    Capability, CatalogEditionTrackList, EditionTrackListRequest, Manifest, PROTOCOL_VERSION,
    RecordingRequest,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};

pub(super) const CAPABILITIES: &str = "v1/catalog/editions/capabilities";
pub(super) const TRACK_LIST: &str = "v1/catalog/editions/track-list";

pub(super) async fn start() -> FakePlugin {
    serde_json::from_value::<CatalogEditionTrackList>(edition())
        .unwrap()
        .validate_for(&EditionTrackListRequest {
            release_group_id: album::GROUP_ID.into(),
            edition_id: album::EDITION_ID.into(),
        })
        .unwrap();
    FakePlugin::builder(Manifest {
        id: "org.hito.library-edition-stats-fixture".into(),
        name: "Owned library edition statistics".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [
            Capability::Catalog,
            Capability::Search,
            Capability::Metadata,
        ]
        .into(),
        authorization: None,
        content_policy: None,
    })
    .get("v1/catalog/capabilities", |_| {
        Reply::Json(json!({
            "catalog_version": 1, "operations": ["artist_profile"]
        }))
    })
    .post("v1/catalog/artists/profile", |request| {
        if request.json() == json!({"artist_id": album::ARTIST_ID}) {
            Reply::Json(json!({
                "artist": {"id": album::ARTIST_ID, "name": album::CREDIT},
                "sort_name": null, "kind": null, "disambiguation": null, "revision": null
            }))
        } else {
            Reply::Refuse
        }
    })
    .get(CAPABILITIES, |_| {
        Reply::Json(json!({
            "edition_version": 1, "operations": ["edition_track_list"]
        }))
    })
    .post(TRACK_LIST, |request| {
        if request.json()
            == json!({"release_group_id": album::GROUP_ID, "edition_id": album::EDITION_ID})
        {
            Reply::Json(edition())
        } else {
            Reply::Refuse
        }
    })
    .post(
        "v1/recordings/detail",
        |request| match serde_json::from_value::<RecordingRequest>(request.json())
            .ok()
            .and_then(|request| album::detail(&request.recording_id))
        {
            Some(recording) => Reply::Json(recording),
            None => Reply::Refuse,
        },
    )
    .strict(24)
    .start()
    .await
}

pub(super) fn edition() -> Value {
    json!({
        "id": album::EDITION_ID, "release_group_id": album::GROUP_ID,
        "title": "Owned selected library edition", "artist_credit": null, "revision": "stats:1",
        "media": [
            {"position": 1, "title": "Main disc", "format": "CD", "tracks": [
                occurrence("pregap", 0, "pregap", Some(2_000), None),
                occurrence("first", 1, "audio", Some(120_000), Some(album::FIRST)),
                occurrence("second", 2, "audio", Some(90_000), Some(album::SECOND)),
                occurrence("data", 3, "data", Some(999_000), None)
            ]},
            {"position": 2, "title": "Bonus disc", "format": "CD", "tracks": [
                occurrence("repeat", 1, "audio", Some(120_000), Some(album::FIRST)),
                occurrence("unknown", 2, "audio", None, Some(album::BONUS))
            ]}
        ]
    })
}

fn occurrence(
    id: &str,
    position: u32,
    kind: &str,
    duration_ms: Option<u64>,
    recording_id: Option<&str>,
) -> Value {
    json!({
        "id": format!("occurrence:library-stats/{id}"), "position": position,
        "number": null, "kind": kind, "title": format!("Owned {id} occurrence"),
        "artist_credit": null, "duration_ms": duration_ms, "recording_id": recording_id
    })
}
