use hito_plugin_sdk::{ArtistProfileRequest, CatalogArtistProfile, MAX_CATALOG_PROFILE_BYTES};
use serde_json::{Value, json};

fn wire_profile() -> Value {
    json!({
        "artist": {"id": "artist:owned/47~á", "name": "Owned Ensemble"},
        "sort_name": null,
        "kind": null,
        "disambiguation": null,
        "revision": null
    })
}

fn accepted(wire: Value, artist_id: &str) -> bool {
    let request = ArtistProfileRequest {
        artist_id: artist_id.to_owned(),
    };
    serde_json::from_value::<CatalogArtistProfile>(wire)
        .is_ok_and(|profile| profile.validate_for(&request).is_ok())
}

#[test]
fn an_artist_profile_preserves_opaque_identity_and_unknown_optional_metadata() {
    let request: ArtistProfileRequest = serde_json::from_value(json!({
        "artist_id": "artist:owned/47~á"
    }))
    .expect("the owned profile request must deserialize");
    request
        .validate()
        .expect("provider artist identities need not be UUIDs");

    let wire = wire_profile();
    let profile: CatalogArtistProfile =
        serde_json::from_value(wire.clone()).expect("the owned artist profile must deserialize");
    profile
        .validate_for(&request)
        .expect("an unknown optional value must not require invented metadata");

    assert_eq!(profile.artist.id, request.artist_id);
    assert_eq!(profile.artist.name, "Owned Ensemble");
    assert!(profile.sort_name.is_none());
    assert!(profile.kind.is_none());
    assert!(profile.disambiguation.is_none());
    assert!(profile.revision.is_none());
    assert_eq!(
        serde_json::to_value(&profile).expect("the owned profile must serialize"),
        wire
    );

    let another: ArtistProfileRequest = serde_json::from_value(json!({
        "artist_id": "artist:owned/another"
    }))
    .expect("another owned profile request must deserialize");
    another
        .validate()
        .expect("another opaque artist identity must be valid");
    assert!(
        profile.validate_for(&another).is_err(),
        "a provider profile must belong to the requested artist"
    );
}

#[test]
fn known_optional_profile_values_round_trip_without_replacing_unknown_values() {
    for kind in [
        "person",
        "group",
        "orchestra",
        "choir",
        "character",
        "other",
    ] {
        let mut wire = wire_profile();
        wire["sort_name"] = json!("Ensemble, Owned");
        wire["kind"] = json!(kind);
        wire["disambiguation"] = json!("Owned fixture ensemble");
        wire["revision"] = json!("owned-revision:2");
        let profile: CatalogArtistProfile =
            serde_json::from_value(wire.clone()).expect("the known owned profile must deserialize");
        profile
            .validate_for(&ArtistProfileRequest {
                artist_id: "artist:owned/47~á".into(),
            })
            .expect("known optional metadata must be valid");
        assert!(profile.kind.is_some());
        assert_eq!(
            serde_json::to_value(&profile).expect("the known profile must serialize"),
            wire
        );
    }
}

#[test]
fn profile_field_limits_count_utf8_bytes() {
    let maximum_id = "é".repeat(128);
    let maximum_text = "é".repeat(1024);
    let mut wire = wire_profile();
    wire["artist"]["id"] = json!(maximum_id);
    wire["artist"]["name"] = json!(maximum_text);
    wire["sort_name"] = json!(maximum_text);
    wire["disambiguation"] = json!(maximum_text);
    wire["revision"] = json!(maximum_id);
    assert!(accepted(wire.clone(), &maximum_id));

    for path in ["/artist/name", "/sort_name", "/disambiguation", "/revision"] {
        let mut oversized = wire.clone();
        let value = oversized
            .pointer_mut(path)
            .expect("the bounded field exists");
        let too_long = format!("{}é", value.as_str().expect("the bounded field is text"));
        *value = json!(too_long);
        assert!(
            !accepted(oversized, &maximum_id),
            "a profile field beyond its UTF-8 byte bound was accepted"
        );
    }

    let oversized_id = format!("{maximum_id}é");
    wire["artist"]["id"] = json!(oversized_id);
    assert!(!accepted(wire, &oversized_id));
    assert!(
        ArtistProfileRequest {
            artist_id: oversized_id
        }
        .validate()
        .is_err()
    );
}

#[test]
fn malformed_profile_fields_and_unrequested_content_are_rejected() {
    for path in [
        "/artist/id",
        "/artist/name",
        "/sort_name",
        "/disambiguation",
        "/revision",
    ] {
        for invalid in [json!(""), json!(" \t"), json!("Owned\nvalue"), json!(17)] {
            let mut wire = wire_profile();
            *wire.pointer_mut(path).expect("the profile field exists") = invalid;
            assert!(
                !accepted(wire, "artist:owned/47~á"),
                "invalid present profile metadata was accepted"
            );
        }
    }
    for kind in [json!("unknown_kind"), json!(7), json!({"name": "group"})] {
        let mut wire = wire_profile();
        wire["kind"] = kind;
        assert!(!accepted(wire, "artist:owned/47~á"));
    }
    for field in ["url", "audio_url", "html", "installation_id", "provider_id"] {
        let mut wire = wire_profile();
        wire[field] = json!("owned unsupported value");
        assert!(!accepted(wire, "artist:owned/47~á"));

        let mut nested = wire_profile();
        nested["artist"][field] = json!("owned unsupported value");
        assert!(!accepted(nested, "artist:owned/47~á"));
    }
    for wire in [
        json!({"artist_id": "artist:owned", "url": "owned unsupported value"}),
        json!({"artist_id": 17}),
        json!({}),
    ] {
        assert!(serde_json::from_value::<ArtistProfileRequest>(wire).is_err());
    }
    for artist_id in ["", " \t", "artist:owned\nvalue"] {
        assert!(
            ArtistProfileRequest {
                artist_id: artist_id.into()
            }
            .validate()
            .is_err()
        );
    }
}

#[test]
fn valid_escaped_profile_fields_fit_the_encoded_profile_budget() {
    assert_eq!(MAX_CATALOG_PROFILE_BYTES, 32_768);
    let artist_id = "\"".repeat(256);
    let mut wire = wire_profile();
    wire["artist"]["id"] = json!(artist_id);
    wire["artist"]["name"] = json!("\"".repeat(2048));
    wire["sort_name"] = json!("\\".repeat(2048));
    wire["disambiguation"] = json!("\"".repeat(2048));
    wire["revision"] = json!("\\".repeat(256));
    let profile: CatalogArtistProfile =
        serde_json::from_value(wire).expect("the escaped profile must deserialize");
    profile
        .validate_for(&ArtistProfileRequest { artist_id })
        .expect("valid escaped fields must fit the profile contract");
    let encoded = serde_json::to_vec(&profile).expect("the escaped profile must serialize");
    assert!(encoded.len() < MAX_CATALOG_PROFILE_BYTES);
}
