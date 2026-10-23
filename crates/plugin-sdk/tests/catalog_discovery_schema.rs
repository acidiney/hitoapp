#![cfg(feature = "schema")]

use hito_plugin_sdk::{
    ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest, CatalogArtistKind,
    CatalogArtistRef, CatalogCapabilities, CatalogDiscoveryCapabilities, CatalogDiscoveryOperation,
    CatalogOperation,
};
use serde_json::{Value, json};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(components(schemas(
    ArtistSearchCandidate,
    ArtistSearchPage,
    ArtistSearchRequest,
    CatalogArtistKind,
    CatalogArtistRef,
    CatalogCapabilities,
    CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation,
    CatalogOperation
)))]
struct DiscoveryContract;

fn accepts_null(schema: &Value) -> bool {
    schema.get("type").is_some_and(|kind| {
        kind == "null"
            || kind
                .as_array()
                .is_some_and(|kinds| kinds.contains(&json!("null")))
    }) || ["anyOf", "oneOf"].iter().any(|key| {
        schema
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|choices| choices.iter().any(accepts_null))
    })
}

#[test]
fn generated_discovery_schema_keeps_nullable_metadata_and_separate_strict_operations() {
    let document = serde_json::to_value(DiscoveryContract::openapi()).unwrap();
    let schemas = &document["components"]["schemas"];
    for (name, fields) in [
        ("ArtistSearchRequest", vec!["cursor", "snapshot"]),
        ("ArtistSearchCandidate", vec!["kind", "disambiguation"]),
        (
            "ArtistSearchPage",
            vec!["snapshot", "total_count", "next_cursor"],
        ),
    ] {
        assert_eq!(schemas[name]["additionalProperties"], json!(false));
        for field in fields {
            assert!(
                accepts_null(&schemas[name]["properties"][field]),
                "Unknown discovery metadata must remain nullable in the generated schema"
            );
        }
    }
    assert_eq!(
        schemas["ArtistSearchRequest"]["required"],
        json!(["query", "limit"])
    );
    assert!(!accepts_null(
        &schemas["ArtistSearchPage"]["properties"]["items"]
    ));
    assert!(!accepts_null(
        &schemas["ArtistSearchCandidate"]["properties"]["artist"]
    ));
    assert_eq!(
        schemas["ArtistSearchPage"]["properties"]["items"]["items"]["$ref"],
        json!("#/components/schemas/ArtistSearchCandidate")
    );
    assert_eq!(
        schemas["ArtistSearchCandidate"]["properties"]["artist"]["$ref"],
        json!("#/components/schemas/CatalogArtistRef")
    );
    assert_eq!(
        schemas["CatalogDiscoveryCapabilities"]["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        schemas["CatalogDiscoveryOperation"]["enum"],
        json!(["artist_search"])
    );
    assert_eq!(
        schemas["CatalogCapabilities"]["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        schemas["CatalogOperation"]["enum"],
        json!(["artist_profile", "artist_discography"])
    );
    assert!(schemas.get("ProviderRecording").is_none());
    assert!(schemas.get("ProviderSource").is_none());
}
