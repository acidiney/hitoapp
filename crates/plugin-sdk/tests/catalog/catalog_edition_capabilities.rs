use hito_plugin_sdk::{
    CATALOG_EDITION_CAPABILITIES_PATH, CATALOG_EDITION_VERSION, CatalogCapabilities,
    CatalogDiscoveryCapabilities, CatalogEditionCapabilities, CatalogEditionOperation,
    CatalogEditionTrackList, EDITION_TRACK_LIST_PATH, EditionTrackListRequest,
};
use serde_json::json;

#[test]
fn edition_negotiation_preserves_existing_descriptors_and_opaque_ids() {
    let catalog_bytes: &[u8] =
        br#"{"catalog_version":1,"operations":["artist_profile","artist_discography"]}"#;
    let catalog: CatalogCapabilities = serde_json::from_slice(catalog_bytes)
        .expect("The existing catalog descriptor must deserialize unchanged");
    catalog
        .validate()
        .expect("The existing catalog descriptor must remain valid");
    assert_eq!(
        serde_json::to_vec(&catalog).expect("The existing catalog descriptor must serialize"),
        catalog_bytes
    );

    let discovery_bytes: &[u8] = br#"{"discovery_version":1,"operations":["artist_search"]}"#;
    let discovery: CatalogDiscoveryCapabilities = serde_json::from_slice(discovery_bytes)
        .expect("The existing discovery descriptor must deserialize unchanged");
    discovery
        .validate()
        .expect("The existing discovery descriptor must remain valid");
    assert_eq!(
        serde_json::to_vec(&discovery).expect("The existing discovery descriptor must serialize"),
        discovery_bytes
    );

    assert_eq!(CATALOG_EDITION_VERSION, 1);
    assert_eq!(
        CATALOG_EDITION_CAPABILITIES_PATH,
        "v1/catalog/editions/capabilities"
    );
    assert_eq!(EDITION_TRACK_LIST_PATH, "v1/catalog/editions/track-list");
    let descriptor_wire = json!({
        "edition_version": 1,
        "operations": ["edition_track_list"]
    });
    let descriptor: CatalogEditionCapabilities = serde_json::from_value(descriptor_wire.clone())
        .expect("The independent edition descriptor must deserialize");
    descriptor
        .validate()
        .expect("The edition track-list operation must be valid");
    assert!(descriptor.operations == vec![CatalogEditionOperation::EditionTrackList]);
    assert_eq!(
        serde_json::to_value(&descriptor).expect("The edition descriptor must serialize"),
        descriptor_wire
    );

    let request_wire = json!({
        "edition_id": "edition:owned/á?pressing=bonus&disc=二",
        "release_group_id": "group:owned/concept#一"
    });
    let request: EditionTrackListRequest = serde_json::from_value(request_wire.clone())
        .expect("Opaque provider edition and release-group identities must deserialize");
    request
        .validate()
        .expect("The shared edition request must accept non-UUID provider identities");
    assert_eq!(
        serde_json::to_value(&request).expect("The opaque edition request must serialize"),
        request_wire
    );

    let edition_wire = json!({
        "id": "edition:owned/á?pressing=bonus&disc=二",
        "release_group_id": "group:owned/concept#一",
        "title": "Owned independent edition",
        "artist_credit": null,
        "revision": null,
        "media": [{
            "position": 1,
            "title": null,
            "format": null,
            "tracks": [{
                "id": "occurrence:owned/disc-1/side-A?number=A1",
                "position": 1,
                "number": "A1",
                "kind": "audio",
                "title": "Owned source-free track",
                "artist_credit": null,
                "duration_ms": null,
                "recording_id": "recording:owned/演奏?take=studio"
            }]
        }]
    });
    let edition: CatalogEditionTrackList = serde_json::from_value(edition_wire.clone())
        .expect("Opaque provider occurrence and recording identities must deserialize");
    edition
        .validate_for(&request)
        .expect("The shared edition response must accept non-UUID provider identities");
    assert_eq!(
        serde_json::to_value(&edition).expect("The independent edition must serialize"),
        edition_wire
    );
}
