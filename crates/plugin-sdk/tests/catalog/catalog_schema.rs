#![cfg(feature = "schema")]

use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CatalogArtistKind,
    CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities, CatalogCreditMember,
    CatalogDiscographyCapabilities, CatalogDiscographyOperation, CatalogOperation,
    CatalogReleaseGroup, CatalogReleaseGroupKind, CatalogReleaseTypeFilter,
};
use serde_json::{Value, json};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(components(schemas(
    ArtistDiscographyPage,
    ArtistDiscographyRequest,
    ArtistProfileRequest,
    CatalogArtistKind,
    CatalogArtistProfile,
    CatalogArtistRef,
    CatalogCapabilities,
    CatalogCreditMember,
    CatalogOperation,
    CatalogReleaseGroup,
    CatalogReleaseGroupKind,
    CatalogReleaseTypeFilter,
    CatalogDiscographyCapabilities,
    CatalogDiscographyOperation
)))]
struct CatalogContract;

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
fn generated_catalog_schema_preserves_nullable_metadata_and_defined_operations() {
    let document = serde_json::to_value(CatalogContract::openapi())
        .expect("the public catalog schema must serialize");
    let schemas = &document["components"]["schemas"];
    let profile = &schemas["CatalogArtistProfile"];
    for field in ["sort_name", "kind", "disambiguation", "revision"] {
        assert!(
            accepts_null(&profile["properties"][field]),
            "an unknown optional profile field must accept null in the schema"
        );
    }
    assert!(!accepts_null(&profile["properties"]["artist"]));
    assert_eq!(profile["additionalProperties"], json!(false));
    assert_eq!(
        schemas["CatalogArtistRef"]["additionalProperties"],
        json!(false)
    );
    assert_eq!(
        schemas["ArtistProfileRequest"]["required"],
        json!(["artist_id"])
    );
    assert_eq!(
        schemas["CatalogOperation"]["enum"],
        json!(["artist_profile", "artist_discography"])
    );
    assert_eq!(
        schemas["CatalogCapabilities"]["additionalProperties"],
        json!(false)
    );
}

#[test]
fn generated_discography_schema_keeps_unknowns_nullable_and_objects_strict() {
    let document = serde_json::to_value(CatalogContract::openapi())
        .expect("the public catalog schema must serialize");
    let schemas = &document["components"]["schemas"];
    for (name, nullable_fields) in [
        ("ArtistDiscographyRequest", vec!["cursor", "snapshot"]),
        (
            "ArtistDiscographyPage",
            vec!["snapshot", "total_count", "next_cursor"],
        ),
        (
            "CatalogReleaseGroup",
            vec!["artist_credit", "kind", "first_release_date", "revision"],
        ),
        ("CatalogCreditMember", vec!["artist"]),
    ] {
        let schema = &schemas[name];
        assert_eq!(schema["additionalProperties"], json!(false));
        for field in nullable_fields {
            assert!(
                accepts_null(&schema["properties"][field]),
                "an unknown catalog value must accept null in the schema"
            );
        }
    }
    assert!(!accepts_null(
        &schemas["ArtistDiscographyPage"]["properties"]["items"]
    ));
    assert!(!accepts_null(
        &schemas["CatalogCreditMember"]["properties"]["credited_name"]
    ));
    assert_eq!(
        schemas["CatalogReleaseGroupKind"]["enum"],
        json!(["album", "ep", "single", "broadcast", "other"])
    );
}

#[test]
fn generated_discography_schema_uses_catalog_groups_and_structured_credit_members() {
    let document = serde_json::to_value(CatalogContract::openapi())
        .expect("the public catalog schema must serialize");
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["ArtistDiscographyPage"]["properties"]["items"]["items"]["$ref"],
        json!("#/components/schemas/CatalogReleaseGroup")
    );
    assert_eq!(
        schemas["CatalogReleaseGroup"]["properties"]["artist_credit"]["items"]["$ref"],
        json!("#/components/schemas/CatalogCreditMember")
    );
    assert_eq!(
        schemas["CatalogCreditMember"]["properties"]["join_phrase"]["type"],
        json!("string")
    );
    assert!(schemas.get("ProviderRecording").is_none());
}

#[test]
fn generated_release_type_filter_schema_is_optional_closed_and_independently_negotiated() {
    let document = serde_json::to_value(CatalogContract::openapi())
        .expect("the public catalog schema must serialize");
    let schemas = &document["components"]["schemas"];
    let request = &schemas["ArtistDiscographyRequest"];
    assert!(
        !request["required"]
            .as_array()
            .expect("the request requires fields")
            .contains(&json!("release_type")),
        "the filter must stay optional so earlier requests remain valid"
    );
    assert_eq!(
        schemas["CatalogReleaseTypeFilter"]["enum"],
        json!(["album", "ep", "single", "other"])
    );
    assert_eq!(
        schemas["CatalogDiscographyOperation"]["enum"],
        json!(["release_type_filter"])
    );
    assert_eq!(
        schemas["CatalogDiscographyCapabilities"]["additionalProperties"],
        json!(false)
    );
}
