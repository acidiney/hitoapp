#[path = "../fixtures/catalog_discography.rs"]
mod fixture;

use hito_plugin_sdk::{
    ArtistDiscographyRequest, CATALOG_DISCOGRAPHY_CAPABILITIES_PATH, CATALOG_DISCOGRAPHY_VERSION,
    CatalogCapabilities, CatalogDiscographyCapabilities, CatalogDiscographyOperation,
    CatalogReleaseTypeFilter,
};
use serde_json::{Value, json};

fn filtered(filter: CatalogReleaseTypeFilter) -> ArtistDiscographyRequest {
    ArtistDiscographyRequest {
        release_type: Some(filter),
        ..fixture::request()
    }
}

#[test]
fn the_filter_is_negotiated_independently_and_leaves_earlier_descriptors_unchanged() {
    assert_eq!(CATALOG_DISCOGRAPHY_VERSION, 1);
    assert_eq!(
        CATALOG_DISCOGRAPHY_CAPABILITIES_PATH,
        "v1/catalog/discography/capabilities"
    );
    let catalog_bytes: &[u8] =
        br#"{"catalog_version":1,"operations":["artist_profile","artist_discography"]}"#;
    let catalog: CatalogCapabilities =
        serde_json::from_slice(catalog_bytes).expect("the earlier catalog descriptor must parse");
    catalog
        .validate()
        .expect("the earlier descriptor stays valid");
    assert_eq!(
        serde_json::to_vec(&catalog).expect("the earlier descriptor must serialize"),
        catalog_bytes
    );

    let wire = json!({"discography_version": 1, "operations": ["release_type_filter"]});
    let descriptor: CatalogDiscographyCapabilities =
        serde_json::from_value(wire.clone()).expect("the filter descriptor must parse");
    descriptor
        .validate()
        .expect("the filter descriptor is valid");
    assert!(descriptor.operations == vec![CatalogDiscographyOperation::ReleaseTypeFilter]);
    assert_eq!(
        serde_json::to_value(&descriptor).expect("the descriptor must serialize"),
        wire
    );

    for invalid in [
        json!({"discography_version": 2, "operations": ["release_type_filter"]}),
        json!({"discography_version": 1, "operations": []}),
        json!({"discography_version": 1, "operations": ["release_type_filter", "release_type_filter"]}),
    ] {
        let parsed: CatalogDiscographyCapabilities =
            serde_json::from_value(invalid).expect("the shape parses before validation");
        assert!(parsed.validate().is_err());
    }
    for unknown in [
        json!({"discography_version": 1, "operations": ["crawl_everything"]}),
        json!({"discography_version": 1, "operations": ["release_type_filter"], "extra": true}),
    ] {
        assert!(serde_json::from_value::<CatalogDiscographyCapabilities>(unknown).is_err());
    }
}

#[test]
fn an_unfiltered_request_keeps_its_earlier_wire_bytes_and_a_filtered_one_names_one_type() {
    let earlier: &[u8] =
        br#"{"artist_id":"artist:owned/47","cursor":null,"snapshot":null,"limit":5}"#;
    let parsed: ArtistDiscographyRequest =
        serde_json::from_slice(earlier).expect("a request without a filter must still parse");
    assert!(parsed.release_type.is_none());
    parsed
        .validate()
        .expect("the unfiltered request stays valid");
    assert_eq!(
        serde_json::to_vec(&parsed).expect("the request must serialize"),
        earlier,
        "an absent filter must not appear on the wire"
    );

    for (filter, name) in [
        (CatalogReleaseTypeFilter::Album, "album"),
        (CatalogReleaseTypeFilter::Ep, "ep"),
        (CatalogReleaseTypeFilter::Single, "single"),
        (CatalogReleaseTypeFilter::Other, "other"),
    ] {
        let request = filtered(filter);
        request.validate().expect("every defined filter is valid");
        let wire = serde_json::to_value(&request).expect("the request must serialize");
        assert_eq!(wire["release_type"], json!(name));
        let round: ArtistDiscographyRequest =
            serde_json::from_value(wire).expect("the filtered request must parse");
        assert!(round.release_type == Some(filter));
    }
    for bad in [
        json!("broadcast"),
        json!("ALBUM"),
        json!(["album"]),
        json!(7),
    ] {
        let mut wire: Value = serde_json::from_slice(earlier).expect("the base request parses");
        wire["release_type"] = bad;
        assert!(serde_json::from_value::<ArtistDiscographyRequest>(wire).is_err());
    }
}

#[test]
fn a_filtered_page_may_only_hold_groups_of_that_type() {
    let cases: [(CatalogReleaseTypeFilter, &[Value], &[Value]); 4] = [
        (
            CatalogReleaseTypeFilter::Album,
            &[json!("album")],
            &[
                json!("ep"),
                json!("single"),
                json!("broadcast"),
                json!("other"),
                Value::Null,
            ],
        ),
        (
            CatalogReleaseTypeFilter::Ep,
            &[json!("ep")],
            &[json!("album"), json!("single"), json!("other"), Value::Null],
        ),
        (
            CatalogReleaseTypeFilter::Single,
            &[json!("single")],
            &[json!("album"), json!("ep"), json!("other"), Value::Null],
        ),
        (
            CatalogReleaseTypeFilter::Other,
            &[json!("broadcast"), json!("other"), Value::Null],
            &[json!("album"), json!("ep"), json!("single")],
        ),
    ];
    for (filter, allowed, refused) in cases {
        let request = filtered(filter);
        for kind in allowed {
            let mut group = fixture::group("group:owned/one");
            group["kind"] = kind.clone();
            assert!(fixture::accepted(fixture::page(vec![group]), &request));
        }
        for kind in refused {
            let mut group = fixture::group("group:owned/one");
            group["kind"] = kind.clone();
            assert!(
                !fixture::accepted(fixture::page(vec![group]), &request),
                "a provider must not answer a typed request with another type"
            );
        }
    }
    let mut group = fixture::group("group:owned/one");
    group["kind"] = json!("ep");
    assert!(fixture::accepted(
        fixture::page(vec![group]),
        &fixture::request()
    ));
}
