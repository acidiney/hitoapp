use hito_plugin_sdk::{
    ArtistProfileRequest, Capability, CatalogArtistProfile, CatalogEditionTrackList,
    CatalogReleaseGroup, EditionTrackListRequest, Manifest, PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{FakePlugin, Reply};
use serde_json::{Value, json};

pub(super) const ARTIST: &str = "artist:owned/header-performer";
pub(super) const GROUP: &str = "group:owned/canonical?edition=none";
pub(super) const EDITION: &str = "edition:owned/bonus?disc=two";
pub(super) const GROUP_CAPABILITIES: &str = "v1/catalog/release-groups/capabilities";
pub(super) const GROUP_DETAIL: &str = "v1/catalog/release-groups/detail";
pub(super) const TRACK_LIST: &str = "v1/catalog/editions/track-list";

pub(super) async fn start() -> FakePlugin {
    group().validate().unwrap();
    edition()
        .validate_for(&EditionTrackListRequest {
            release_group_id: GROUP.into(),
            edition_id: EDITION.into(),
        })
        .unwrap();
    FakePlugin::builder(Manifest {
        id: "org.hito.cold-album-header-fixture".into(),
        name: "Owned cold album header catalog".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Catalog].into(),
        authorization: None,
        content_policy: None,
    })
    .get("v1/catalog/capabilities", |_| {
        Reply::Json(json!({"catalog_version": 1, "operations": ["artist_profile"]}))
    })
    .post(
        "v1/catalog/artists/profile",
        |request| match serde_json::from_value::<ArtistProfileRequest>(request.json()) {
            Ok(request) if request.validate().is_ok() && request.artist_id == ARTIST => {
                Reply::Json(json!(profile()))
            }
            _ => Reply::Refuse,
        },
    )
    .get("v1/catalog/editions/capabilities", |_| {
        Reply::Json(json!({"edition_version": 1, "operations": ["edition_track_list"]}))
    })
    .get(GROUP_CAPABILITIES, |_| {
        Reply::Json(json!({"release_group_version": 1, "operations": ["release_group"]}))
    })
    .post(GROUP_DETAIL, |request| {
        if request.json() == json!({"release_group_id": GROUP}) {
            Reply::Json(json!(group()))
        } else {
            Reply::Refuse
        }
    })
    .post(TRACK_LIST, |request| {
        match serde_json::from_value::<EditionTrackListRequest>(request.json()) {
            Ok(request)
                if request.validate().is_ok()
                    && request.release_group_id == GROUP
                    && request.edition_id == EDITION =>
            {
                Reply::Json(json!(edition()))
            }
            _ => Reply::Refuse,
        }
    })
    .strict(24)
    .start()
    .await
}

pub(super) fn group() -> CatalogReleaseGroup {
    serde_json::from_value(json!({
        "id": GROUP,
        "title": "Canonical owned album",
        "artist_credit": [{
            "artist": {"id": ARTIST, "name": "Owned header performer"},
            "credited_name": "Owned header performer", "join_phrase": ""
        }],
        "kind": "album", "secondary_types": ["Soundtrack"],
        "first_release_date": "1998-03-17", "revision": "owned-group:4"
    }))
    .unwrap()
}

pub(super) fn edition() -> CatalogEditionTrackList {
    serde_json::from_value(json!({
        "id": EDITION, "release_group_id": GROUP,
        "title": "Owned bonus edition from 2019", "artist_credit": null,
        "revision": "owned-edition:2",
        "media": [{"position": 1, "title": "Main disc", "format": "CD", "tracks": [
            occurrence("pregap", 0, "pregap", Some(2000), None),
            occurrence("main", 1, "audio", Some(120000), Some("recording:owned/repeated")),
            occurrence("unknown", 2, "audio", None, None),
            occurrence("data", 3, "data", Some(999000), None)
        ]}, {"position": 2, "title": "Bonus disc", "format": "CD", "tracks": [
            occurrence("repeat", 1, "audio", Some(120000), Some("recording:owned/repeated")),
            occurrence("bonus", 2, "audio", Some(45000), Some("recording:owned/bonus"))
        ]}]
    }))
    .unwrap()
}

fn occurrence(
    id: &str,
    position: u32,
    kind: &str,
    duration_ms: Option<u64>,
    recording_id: Option<&str>,
) -> Value {
    json!({
        "id": format!("occurrence:owned/{id}"), "position": position, "number": null,
        "kind": kind, "title": format!("Owned {id} occurrence"), "artist_credit": null,
        "duration_ms": duration_ms, "recording_id": recording_id
    })
}

fn profile() -> CatalogArtistProfile {
    serde_json::from_value(json!({
        "artist": {"id": ARTIST, "name": "Owned header performer"},
        "sort_name": null, "kind": null, "disambiguation": null, "revision": null
    }))
    .unwrap()
}
