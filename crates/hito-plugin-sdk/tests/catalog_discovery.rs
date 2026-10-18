use hito_plugin_sdk::{
    ARTIST_SEARCH_PATH, ArtistSearchPage, ArtistSearchRequest, CATALOG_DISCOVERY_CAPABILITIES_PATH,
    CATALOG_DISCOVERY_VERSION, CatalogCapabilities, CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation, CatalogOperation,
};
use serde_json::json;

#[test]
fn artist_discovery_preserves_distinct_candidates_and_the_legacy_catalog_descriptor() {
    let legacy_bytes: &[u8] =
        br#"{"catalog_version":1,"operations":["artist_profile","artist_discography"]}"#;
    let legacy: CatalogCapabilities = serde_json::from_slice(legacy_bytes)
        .expect("The existing catalog descriptor must deserialize unchanged");
    legacy
        .validate()
        .expect("The existing catalog operations must remain valid");
    assert!(
        legacy.operations
            == vec![
                CatalogOperation::ArtistProfile,
                CatalogOperation::ArtistDiscography,
            ]
    );
    assert_eq!(
        serde_json::to_vec(&legacy).expect("The existing descriptor must serialize"),
        legacy_bytes,
        "Artist discovery must not change the existing catalog wire bytes"
    );

    assert_eq!(CATALOG_DISCOVERY_VERSION, 1);
    assert_eq!(
        CATALOG_DISCOVERY_CAPABILITIES_PATH,
        "v1/catalog/discovery/capabilities"
    );
    assert_eq!(ARTIST_SEARCH_PATH, "v1/catalog/artists/search");
    let descriptor_wire = json!({
        "discovery_version": 1,
        "operations": ["artist_search"]
    });
    let descriptor: CatalogDiscoveryCapabilities = serde_json::from_value(descriptor_wire.clone())
        .expect("The separate discovery descriptor must deserialize");
    descriptor
        .validate()
        .expect("Artist search must be a valid discovery operation");
    assert!(descriptor.operations == vec![CatalogDiscoveryOperation::ArtistSearch]);
    assert_eq!(
        serde_json::to_value(&descriptor).expect("The discovery descriptor must serialize"),
        descriptor_wire
    );

    let request_wire = json!({
        "query": "Owned Ensemble & invités",
        "cursor": null,
        "snapshot": null,
        "limit": 5
    });
    let request: ArtistSearchRequest = serde_json::from_value(request_wire.clone())
        .expect("The first owned artist search must deserialize");
    request
        .validate()
        .expect("The first owned artist search must be valid");
    assert_eq!(
        serde_json::to_value(&request).expect("The artist search must serialize"),
        request_wire
    );

    let page_wire = json!({
        "query": "Owned Ensemble & invités",
        "snapshot": "owned-search-snapshot:1",
        "items": [
            {
                "artist": {"id": "artist:owned/47~á", "name": "Owned Ensemble"},
                "kind": "group",
                "disambiguation": "Owned acoustic ensemble"
            },
            {
                "artist": {"id": "artist:owned/another?credit=lead", "name": "Owned Ensemble"},
                "kind": null,
                "disambiguation": null
            }
        ],
        "total_count": null,
        "next_cursor": "owned-search-cursor:next/2"
    });
    let page: ArtistSearchPage = serde_json::from_value(page_wire.clone())
        .expect("The owned artist candidate page must deserialize");
    page.validate_for(&request)
        .expect("Distinct artists with equal names must remain separate candidates");
    assert_eq!(page.query, request.query);
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].artist.name, page.items[1].artist.name);
    assert_ne!(page.items[0].artist.id, page.items[1].artist.id);
    assert_eq!(page.items[0].artist.id, "artist:owned/47~á");
    assert_eq!(page.items[1].artist.id, "artist:owned/another?credit=lead");
    assert!(page.items[1].kind.is_none());
    assert!(page.items[1].disambiguation.is_none());
    assert!(page.total_count.is_none());
    assert_eq!(
        serde_json::to_value(&page).expect("The candidate page must serialize"),
        page_wire
    );

    let continuation = ArtistSearchRequest {
        query: request.query,
        cursor: page.next_cursor,
        snapshot: page.snapshot,
        limit: request.limit,
    };
    continuation
        .validate()
        .expect("The selected provider's opaque search continuation must be valid");
    let final_wire = json!({
        "query": "Owned Ensemble & invités",
        "snapshot": "owned-search-snapshot:1",
        "items": [],
        "total_count": null,
        "next_cursor": null
    });
    let final_page: ArtistSearchPage = serde_json::from_value(final_wire.clone())
        .expect("The owned terminal candidate page must deserialize");
    final_page
        .validate_for(&continuation)
        .expect("The terminal page must retain the requested query and provider snapshot");
    assert_eq!(
        serde_json::to_value(&final_page).expect("The terminal candidate page must serialize"),
        final_wire
    );
}
