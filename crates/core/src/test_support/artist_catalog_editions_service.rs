pub use super::observed::ObservedRequest;
use super::observed::observed;
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CATALOG_EDITION_VERSION,
    CATALOG_VERSION, Capability, CatalogCapabilities, CatalogEditionCapabilities,
    CatalogEditionOperation, CatalogEditionSummary, CatalogEditionTrackList, CatalogOperation,
    EditionTrackListRequest, Manifest, PROTOCOL_VERSION, ReleaseGroupEditionsPage,
    ReleaseGroupEditionsRequest,
};
pub use hito_testkit::fake_plugin::Gate as EditionGate;
use hito_testkit::fake_plugin::{self as fake, FakePlugin};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const CAPABILITIES: &str = "v1/catalog/capabilities";
const PROFILE: &str = "v1/catalog/artists/profile";
const DISCOGRAPHY: &str = "v1/catalog/artists/discography";
const EDITION_CAPABILITIES: &str = "v1/catalog/editions/capabilities";
const EDITIONS: &str = "v1/catalog/release-groups/editions";
const TRACK_LIST: &str = "v1/catalog/editions/track-list";
const REQUEST_LIMIT: usize = 32;

pub const ARTIST_ID: &str = "artist:owned/演奏?credit=lead";
pub const GROUP_ID: &str = "group:owned/á?album=一&credit=guest";
pub const BONUS_ID: &str = "edition:owned/bonus?pressing=二";
pub const PROVIDER_CURSOR: &str = "cursor:owned/次?edition=2&signature=private";
const STANDARD_ID: &str = "edition:owned/standard?pressing=一";
const RECORDING_ID: &str = "recording:owned/演奏?take=studio";

pub struct CatalogServer {
    pub url: String,
    plugin: FakePlugin,
    listing_enabled: Arc<AtomicBool>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let listing_enabled = Arc::new(AtomicBool::new(false));
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.artist-editions-fixture".into(),
            name: "Owned artist editions".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .get(CAPABILITIES, |_| {
            fake::Reply::Json(
                serde_json::to_value(CatalogCapabilities {
                    catalog_version: CATALOG_VERSION,
                    operations: vec![
                        CatalogOperation::ArtistProfile,
                        CatalogOperation::ArtistDiscography,
                    ],
                })
                .unwrap(),
            )
        })
        .post(PROFILE, |request| {
            match serde_json::from_value::<ArtistProfileRequest>(request.json()) {
                Ok(parsed) if parsed.validate().is_ok() && parsed.artist_id == ARTIST_ID => {
                    fake::Reply::Json(
                        json!({"artist": {"id": ARTIST_ID, "name": "Owned performer"},
                        "sort_name": null, "kind": null, "disambiguation": null, "revision": null}),
                    )
                }
                _ => fake::Reply::Refuse,
            }
        })
        .post(DISCOGRAPHY, |request| {
            match serde_json::from_value::<ArtistDiscographyRequest>(request.json()) {
                Ok(parsed)
                    if parsed.validate().is_ok()
                        && parsed.artist_id == ARTIST_ID
                        && parsed.cursor.is_none()
                        && parsed.snapshot.is_none()
                        && parsed.limit == 1 =>
                {
                    fake::Reply::Json(serde_json::to_value(expected_discography()).unwrap())
                }
                _ => fake::Reply::Refuse,
            }
        })
        .get(EDITION_CAPABILITIES, {
            let enabled = listing_enabled.clone();
            move |_| {
                let mut operations = vec![CatalogEditionOperation::EditionTrackList];
                if enabled.load(Ordering::SeqCst) {
                    operations.push(CatalogEditionOperation::ReleaseGroupEditions);
                }
                fake::Reply::Json(
                    serde_json::to_value(CatalogEditionCapabilities {
                        edition_version: CATALOG_EDITION_VERSION,
                        operations,
                    })
                    .unwrap(),
                )
            }
        })
        .post(EDITIONS, {
            let enabled = listing_enabled.clone();
            move |request| match serde_json::from_value::<ReleaseGroupEditionsRequest>(
                request.json(),
            ) {
                Ok(parsed)
                    if enabled.load(Ordering::SeqCst)
                        && parsed.validate().is_ok()
                        && parsed.release_group_id == GROUP_ID
                        && parsed.limit == 1
                        && parsed.snapshot.is_none()
                        && matches!(parsed.cursor.as_deref(), None | Some(PROVIDER_CURSOR)) =>
                {
                    fake::Reply::Json(
                        serde_json::to_value(expected_editions(parsed.cursor.is_some())).unwrap(),
                    )
                }
                _ => fake::Reply::Refuse,
            }
        })
        .post(TRACK_LIST, |request| {
            match serde_json::from_value::<EditionTrackListRequest>(request.json()) {
                Ok(parsed)
                    if parsed.validate().is_ok()
                        && parsed.release_group_id == GROUP_ID
                        && parsed.edition_id == BONUS_ID =>
                {
                    fake::Reply::Json(serde_json::to_value(expected_track_list()).unwrap())
                }
                _ => fake::Reply::Refuse,
            }
        })
        .strict(REQUEST_LIMIT)
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            listing_enabled,
        }
    }

    pub fn enable_edition_listing(&self) {
        self.listing_enabled.store(true, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        observed(&self.plugin, REQUEST_LIMIT)
    }

    pub fn unexpected(&self) -> usize {
        self.plugin.unexpected()
    }

    pub fn hold(&self, operation: CatalogEditionOperation) -> Arc<EditionGate> {
        let path = match operation {
            CatalogEditionOperation::ReleaseGroupEditions => EDITIONS,
            CatalogEditionOperation::EditionTrackList => TRACK_LIST,
        };
        self.plugin.hold_next(path)
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}

fn expected_discography() -> ArtistDiscographyPage {
    serde_json::from_value(json!({
        "artist_id": ARTIST_ID, "snapshot": null, "total_count": 1, "next_cursor": null,
        "items": [{"id": GROUP_ID, "title": "Owned album", "artist_credit": null,
            "kind": "album", "secondary_types": [], "first_release_date": null, "revision": null}]
    }))
    .unwrap()
}

pub fn expected_editions(last: bool) -> ReleaseGroupEditionsPage {
    ReleaseGroupEditionsPage {
        release_group_id: GROUP_ID.into(),
        snapshot: None,
        items: vec![CatalogEditionSummary {
            id: if last { BONUS_ID } else { STANDARD_ID }.into(),
            title: "Owned album".into(),
            artist_credit: None,
            disambiguation: Some(
                if last {
                    "bonus disc"
                } else {
                    "standard edition"
                }
                .into(),
            ),
            release_date: None,
            country: None,
            revision: None,
        }],
        total_count: Some(2),
        next_cursor: (!last).then(|| PROVIDER_CURSOR.into()),
    }
}

pub fn expected_track_list() -> CatalogEditionTrackList {
    serde_json::from_value(json!({
        "id": BONUS_ID, "release_group_id": GROUP_ID, "title": "Owned album",
        "artist_credit": null, "revision": null,
        "media": [
            {"position": 1, "title": "Main disc", "format": "CD", "tracks": [
                {"id": "occurrence:owned/disc-1/main?number=A1", "position": 1, "number": "A1",
                    "kind": "audio", "title": "Owned main track", "artist_credit": null,
                    "duration_ms": 120000, "recording_id": RECORDING_ID},
                {"id": "occurrence:owned/disc-1/second?number=A2", "position": 2, "number": "A2",
                    "kind": "audio", "title": "Owned second track", "artist_credit": null,
                    "duration_ms": null, "recording_id": null}
            ]},
            {"position": 2, "title": "Bonus disc", "format": null, "tracks": [
                {"id": "occurrence:owned/disc-2/repeat?number=1", "position": 1, "number": "1",
                    "kind": "audio", "title": "Owned repeated occurrence", "artist_credit": null,
                    "duration_ms": 120000, "recording_id": RECORDING_ID},
                {"id": "occurrence:owned/disc-2/bonus?number=Bonus", "position": 2, "number": "Bonus",
                    "kind": "audio", "title": "Owned bonus track", "artist_credit": null,
                    "duration_ms": null, "recording_id": null}
            ]}
        ]
    })).unwrap()
}
