pub use super::observed::ObservedRequest;
use super::observed::observed;
use hito_plugin_sdk::{
    ArtistProfileRequest, CATALOG_EDITION_VERSION, CATALOG_VERSION, Capability,
    CatalogCapabilities, CatalogEditionCapabilities, CatalogEditionOperation,
    EditionTrackListRequest, Manifest, PROTOCOL_VERSION, RecordingRequest,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const REQUEST_LIMIT: usize = 64;

pub const ARTIST_ID: &str = "artist:album/owned?credit=lead";
pub const CREDIT: &str = "Owned album performer";
pub const GROUP_ID: &str = "group:album/owned?kind=album";
pub const EDITION_ID: &str = "edition:album/deluxe?pressing=二";
pub const STANDARD_ID: &str = "edition:album/standard?pressing=一";
pub const RELEASE_TITLE: &str = "Owned album";
pub const FIRST: &str = "recording:album/first";
pub const SECOND: &str = "recording:album/second";
pub const EXTRA: &str = "recording:album/library-only";
pub const BONUS: &str = "recording:album/bonus";
pub const FEATURED: &str = "recording:album/featured-guest";
pub const FEATURED_CREDIT: &str = "Featured guest performer";
pub const TWIN: &str = "recording:album/same-title-other-recording";
pub const FIRST_TITLE: &str = "Shared title";

struct Behavior {
    outage: AtomicBool,
    editions: AtomicBool,
    bonus: Mutex<(String, Option<u64>)>,
}

pub struct AlbumProvider {
    pub url: String,
    plugin: FakePlugin,
    behavior: Arc<Behavior>,
}

impl AlbumProvider {
    pub async fn start() -> Self {
        let behavior = Arc::new(Behavior {
            outage: AtomicBool::new(false),
            editions: AtomicBool::new(true),
            bonus: Mutex::new((CREDIT.into(), Some(1000))),
        });
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.album-edition-fixture".into(),
            name: "Owned album editions".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog, Capability::Search, Capability::Metadata].into(),
            authorization: None,
            content_policy: None,
        })
        .get("v1/catalog/capabilities", |_| {
            fake::Reply::Json(
                serde_json::to_value(CatalogCapabilities {
                    catalog_version: CATALOG_VERSION,
                    operations: vec![
                        hito_plugin_sdk::CatalogOperation::ArtistProfile,
                        hito_plugin_sdk::CatalogOperation::ArtistDiscography,
                    ],
                })
                .unwrap(),
            )
        })
        .get("v1/catalog/editions/capabilities", {
            let behavior = behavior.clone();
            move |_| {
                let operations = if behavior.editions.load(Ordering::SeqCst) {
                    vec![
                        CatalogEditionOperation::ReleaseGroupEditions,
                        CatalogEditionOperation::EditionTrackList,
                    ]
                } else {
                    vec![CatalogEditionOperation::ReleaseGroupEditions]
                };
                fake::Reply::Json(
                    serde_json::to_value(CatalogEditionCapabilities {
                        edition_version: CATALOG_EDITION_VERSION,
                        operations,
                    })
                    .unwrap(),
                )
            }
        })
        .post("v1/catalog/artists/profile", |request| {
            match serde_json::from_value::<ArtistProfileRequest>(request.json()) {
                Ok(parsed) if parsed.artist_id == ARTIST_ID => fake::Reply::Json(json!({
                    "artist": {"id": ARTIST_ID, "name": CREDIT},
                    "sort_name": null, "kind": null, "disambiguation": null, "revision": null
                })),
                _ => fake::Reply::Refuse,
            }
        })
        .post("v1/catalog/artists/discography", |_| {
            fake::Reply::Json(json!({
                "artist_id": ARTIST_ID, "snapshot": null, "total_count": 1, "next_cursor": null,
                "items": [{"id": GROUP_ID, "title": RELEASE_TITLE, "artist_credit": null,
                    "kind": "album", "secondary_types": [], "first_release_date": null, "revision": null}]
            }))
        })
        .post("v1/catalog/release-groups/editions", |_| {
            fake::Reply::Json(json!({
                "release_group_id": GROUP_ID, "snapshot": null, "total_count": 1, "next_cursor": null,
                "items": [{"id": EDITION_ID, "title": RELEASE_TITLE, "artist_credit": null,
                    "disambiguation": "deluxe", "release_date": null, "country": null, "revision": null}]
            }))
        })
        .post("v1/catalog/editions/track-list", {
            let behavior = behavior.clone();
            move |request| {
                let Ok(parsed) = serde_json::from_value::<EditionTrackListRequest>(request.json())
                else {
                    return fake::Reply::Refuse;
                };
                let list = if parsed.edition_id == EDITION_ID {
                    Some(deluxe(&behavior.bonus.lock().unwrap().clone()))
                } else {
                    track_list(&parsed.edition_id)
                };
                let Some(list) = list.filter(|_| parsed.release_group_id == GROUP_ID) else {
                    return fake::Reply::Refuse;
                };
                if behavior.outage.load(Ordering::SeqCst) {
                    return fake::Reply::Status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
                }
                fake::Reply::Json(list)
            }
        })
        .post("v1/recordings/detail", |request| {
            match serde_json::from_value::<RecordingRequest>(request.json())
                .ok()
                .and_then(|parsed| detail(&parsed.recording_id))
            {
                Some(value) => fake::Reply::Json(value),
                None => fake::Reply::Refuse,
            }
        })
        .strict(REQUEST_LIMIT)
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            behavior,
        }
    }

    pub fn set_outage(&self, outage: bool) {
        self.behavior.outage.store(outage, Ordering::SeqCst);
    }

    /// Changes the credit and duration the edition reports for the bonus track.
    pub fn set_bonus(&self, credit: &str, duration_ms: Option<u64>) {
        *self.behavior.bonus.lock().unwrap() = (credit.into(), duration_ms);
    }

    pub fn set_editions(&self, supported: bool) {
        self.behavior.editions.store(supported, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        observed(&self.plugin, REQUEST_LIMIT)
    }

    pub fn requests_to(&self, path: &str) -> Vec<ObservedRequest> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == format!("/prefix/{path}"))
            .collect()
    }

    pub fn unexpected(&self) -> usize {
        self.plugin.unexpected()
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}

pub fn detail(id: &str) -> Option<Value> {
    let (title, disc, track, artist) = match id {
        FIRST => (FIRST_TITLE, 1, 2, CREDIT),
        SECOND => ("Second provider title", 1, 3, CREDIT),
        EXTRA => ("Library only track", 1, 9, CREDIT),
        FEATURED => ("Featured provider title", 1, 1, FEATURED_CREDIT),
        BONUS => ("Bonus provider title", 2, 2, CREDIT),
        _ => return None,
    };
    Some(json!({
        "id": id, "title": title, "artist": artist, "duration_ms": 1000,
        "release": {"id": EDITION_ID, "title": RELEASE_TITLE, "artist": CREDIT},
        "disc_number": disc, "track_number": track, "revision": "album-r1"
    }))
}

pub fn track_list(edition: &str) -> Option<Value> {
    match edition {
        EDITION_ID => Some(deluxe(&(CREDIT.into(), Some(1000)))),
        STANDARD_ID => Some(json!({
            "id": STANDARD_ID, "release_group_id": GROUP_ID, "title": RELEASE_TITLE,
            "artist_credit": null, "revision": null,
            "media": [{"position": 1, "title": null, "format": "CD", "tracks": [
                {"id": "occurrence:standard/1/1", "position": 1, "number": "1", "kind": "audio",
                    "title": "Standard first", "artist_credit": null, "duration_ms": 1000,
                    "recording_id": FIRST}
            ]}]
        })),
        _ => None,
    }
}

fn deluxe(bonus: &(String, Option<u64>)) -> Value {
    let occurrence = |id: &str, position: u32, kind: &str, title: &str, recording: Option<&str>| {
        let (credit, duration) = if id == "occurrence:album/2/2" {
            (bonus.0.as_str(), bonus.1)
        } else {
            (CREDIT, Some(1000))
        };
        json!({
            "id": id, "position": position,
            "number": if position == 0 { Value::Null } else { json!(position.to_string()) },
            "kind": kind, "title": title,
            "artist_credit": [{"artist": null, "credited_name": credit, "join_phrase": ""}],
            "duration_ms": duration, "recording_id": recording
        })
    };
    json!({
        "id": EDITION_ID, "release_group_id": GROUP_ID, "title": RELEASE_TITLE,
        "artist_credit": null, "revision": null,
        "media": [
            {"position": 1, "title": "Main disc", "format": "CD", "tracks": [
                occurrence("occurrence:album/1/0", 0, "pregap", "Hidden intro", None),
                occurrence("occurrence:album/1/1", 1, "audio", "First catalog title", Some(FIRST)),
                occurrence("occurrence:album/1/2", 2, "audio", "Second catalog title", Some(SECOND)),
                occurrence("occurrence:album/1/3", 3, "audio", FIRST_TITLE, Some(TWIN))
            ]},
            {"position": 2, "title": "Bonus disc", "format": null, "tracks": [
                occurrence("occurrence:album/2/1", 1, "audio", "First catalog title", Some(FIRST)),
                occurrence("occurrence:album/2/2", 2, "audio", "Bonus catalog title", Some(BONUS)),
                occurrence("occurrence:album/2/3", 3, "audio", "Unidentified bonus", None)
            ]}
        ]
    })
}
