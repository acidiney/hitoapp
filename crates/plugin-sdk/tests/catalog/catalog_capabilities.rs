use hito_plugin_sdk::{
    ARTIST_DISCOGRAPHY_PATH, ARTIST_PROFILE_PATH, CATALOG_CAPABILITIES_PATH, CATALOG_VERSION,
    CatalogCapabilities, CatalogOperation,
};
use serde_json::{Value, json};

fn accepted(wire: Value) -> bool {
    serde_json::from_value::<CatalogCapabilities>(wire)
        .is_ok_and(|capabilities| capabilities.validate().is_ok())
}

#[test]
fn a_catalog_descriptor_declares_only_defined_read_only_operations() {
    assert_eq!(CATALOG_VERSION, 1);
    assert_eq!(CATALOG_CAPABILITIES_PATH, "v1/catalog/capabilities");
    assert_eq!(ARTIST_PROFILE_PATH, "v1/catalog/artists/profile");
    assert_eq!(ARTIST_DISCOGRAPHY_PATH, "v1/catalog/artists/discography");

    let wire = json!({
        "catalog_version": 1,
        "operations": ["artist_profile", "artist_discography"]
    });
    let capabilities: CatalogCapabilities = serde_json::from_value(wire.clone())
        .expect("the owned catalog descriptor must deserialize");
    capabilities
        .validate()
        .expect("defined catalog operations must be valid");
    assert!(
        capabilities.operations
            == vec![
                CatalogOperation::ArtistProfile,
                CatalogOperation::ArtistDiscography
            ]
    );
    assert_eq!(
        serde_json::to_value(&capabilities).expect("the owned descriptor must serialize"),
        wire
    );
    for operations in [
        json!(["artist_profile"]),
        json!(["artist_discography"]),
        json!(["artist_discography", "artist_profile"]),
    ] {
        assert!(accepted(json!({
            "catalog_version": 1,
            "operations": operations
        })));
    }

    assert!(!accepted(json!({
        "catalog_version": 2,
        "operations": ["artist_profile"]
    })));
    assert!(!accepted(json!({
        "catalog_version": 1,
        "operations": ["artist_profile", "artist_profile"]
    })));
}

#[test]
fn malformed_or_undefined_catalog_declarations_fail_closed() {
    for wire in [
        json!({"catalog_version": 0, "operations": ["artist_profile"]}),
        json!({"catalog_version": 1, "operations": []}),
        json!({"catalog_version": 1, "operations": ["artist_similarity"]}),
        json!({"catalog_version": 1, "operations": ["release_tracklist"]}),
        json!({"catalog_version": 1, "operations": ["artist_profile", null]}),
        json!({"catalog_version": "1", "operations": ["artist_profile"]}),
        json!({"catalog_version": 1, "operations": "artist_profile"}),
        json!({"operations": ["artist_profile"]}),
        json!({"catalog_version": 1}),
        json!({
            "catalog_version": 1,
            "operations": ["artist_profile"],
            "html": "owned unsupported field"
        }),
    ] {
        assert!(
            !accepted(wire),
            "an invalid catalog declaration was accepted"
        );
    }
}
