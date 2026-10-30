use hito_plugin_sdk::{
    ARTIST_SIMILARITY_PATH, ArtistSimilarityPage, ArtistSimilarityRequest,
    CATALOG_SIMILARITY_CAPABILITIES_PATH, CATALOG_SIMILARITY_VERSION, CatalogArtistRef,
    CatalogCapabilities, CatalogSimilarityCapabilities, CatalogSimilarityIdentity,
    CatalogSimilarityOperation, MAX_SIMILAR_ARTISTS, MAX_SIMILAR_ARTISTS_BYTES, SimilarArtist,
};
use serde_json::json;

const SEED: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";

fn request(limit: u16) -> ArtistSimilarityRequest {
    ArtistSimilarityRequest {
        artist_id: SEED.into(),
        limit,
    }
}

fn similar(id: &str, name: &str, score: Option<f64>) -> SimilarArtist {
    SimilarArtist {
        artist: CatalogArtistRef {
            id: id.into(),
            name: name.into(),
        },
        score,
    }
}

fn page(items: Vec<SimilarArtist>) -> ArtistSimilarityPage {
    ArtistSimilarityPage {
        artist_id: SEED.into(),
        items,
    }
}

#[test]
fn the_similarity_descriptor_is_independent_and_leaves_the_catalog_descriptor_unchanged() {
    let catalog_bytes: &[u8] = br#"{"catalog_version":1,"operations":["artist_profile"]}"#;
    let catalog: CatalogCapabilities = serde_json::from_slice(catalog_bytes).unwrap();
    catalog.validate().unwrap();
    assert_eq!(serde_json::to_vec(&catalog).unwrap(), catalog_bytes);

    assert_eq!(CATALOG_SIMILARITY_VERSION, 1);
    assert_eq!(
        CATALOG_SIMILARITY_CAPABILITIES_PATH,
        "v1/catalog/similarity/capabilities"
    );
    assert_eq!(ARTIST_SIMILARITY_PATH, "v1/catalog/similarity/artists");
    let wire = json!({
        "similarity_version": 1,
        "operations": ["artist_similarity"],
        "artist_identity": "musicbrainz_artist"
    });
    let descriptor: CatalogSimilarityCapabilities = serde_json::from_value(wire.clone()).unwrap();
    descriptor.validate().unwrap();
    assert!(descriptor.operations == vec![CatalogSimilarityOperation::ArtistSimilarity]);
    assert!(descriptor.artist_identity == CatalogSimilarityIdentity::MusicbrainzArtist);
    assert_eq!(serde_json::to_value(&descriptor).unwrap(), wire);
}

#[test]
fn descriptors_with_the_wrong_version_operations_or_identity_are_rejected() {
    for wire in [
        json!({"similarity_version": 2, "operations": ["artist_similarity"], "artist_identity": "musicbrainz_artist"}),
        json!({"similarity_version": 1, "operations": [], "artist_identity": "musicbrainz_artist"}),
        json!({"similarity_version": 1, "operations": ["artist_similarity", "artist_similarity"], "artist_identity": "musicbrainz_artist"}),
    ] {
        let descriptor: CatalogSimilarityCapabilities = serde_json::from_value(wire).unwrap();
        assert!(descriptor.validate().is_err());
    }
    for wire in [
        json!({"similarity_version": 1, "operations": ["artist_similarity"], "artist_identity": "name"}),
        json!({"similarity_version": 1, "operations": ["artist_similarity"]}),
        json!({"similarity_version": 1, "operations": ["listening_history"], "artist_identity": "musicbrainz_artist"}),
        json!({"similarity_version": 1, "operations": ["artist_similarity"], "artist_identity": "musicbrainz_artist", "extra": true}),
    ] {
        assert!(serde_json::from_value::<CatalogSimilarityCapabilities>(wire).is_err());
    }
}

#[test]
fn a_request_carries_only_an_identity_and_a_bounded_limit() {
    let wire = json!({"artist_id": SEED, "limit": 10});
    let parsed: ArtistSimilarityRequest = serde_json::from_value(wire.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
    assert!(
        serde_json::from_value::<ArtistSimilarityRequest>(
            json!({"artist_id": SEED, "limit": 10, "listens": []})
        )
        .is_err()
    );
    assert!(request(0).validate().is_err());
    assert!(request(MAX_SIMILAR_ARTISTS + 1).validate().is_err());
    request(MAX_SIMILAR_ARTISTS).validate().unwrap();
    assert!(
        ArtistSimilarityRequest {
            artist_id: String::new(),
            limit: 5
        }
        .validate()
        .is_err()
    );
    assert!(
        ArtistSimilarityRequest {
            artist_id: "x".repeat(257),
            limit: 5
        }
        .validate()
        .is_err()
    );
}

#[test]
fn a_valid_page_keeps_provider_order_and_optional_scores() {
    let wire = json!({
        "artist_id": SEED,
        "items": [
            {"artist": {"id": OTHER, "name": "Segunda Banda"}, "score": 0.5},
            {"artist": {"id": "33333333-3333-4333-8333-333333333333", "name": "Terceira"}, "score": null}
        ]
    });
    let parsed: ArtistSimilarityPage = serde_json::from_value(wire.clone()).unwrap();
    parsed.validate_for(&request(5)).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), wire);
    page(vec![]).validate_for(&request(5)).unwrap();
    assert!(
        serde_json::from_value::<ArtistSimilarityPage>(
            json!({"artist_id": SEED, "items": [], "next_cursor": "x"})
        )
        .is_err()
    );
}

#[test]
fn a_page_for_another_artist_or_with_repeated_or_self_identities_is_rejected() {
    let ok = || similar(OTHER, "Segunda Banda", Some(1.0));
    assert!(
        ArtistSimilarityPage {
            artist_id: OTHER.into(),
            items: vec![ok()]
        }
        .validate_for(&request(5))
        .is_err()
    );
    assert!(
        page(vec![ok(), ok()]).validate_for(&request(5)).is_err(),
        "an identity may appear once"
    );
    assert!(
        page(vec![similar(SEED, "Mesma", None)])
            .validate_for(&request(5))
            .is_err(),
        "an artist is never similar to itself"
    );
    assert!(
        page(vec![ok(), similar("33", "Outra", None)])
            .validate_for(&request(1))
            .is_err(),
        "more rows than requested"
    );
}

#[test]
fn names_identities_and_scores_are_bounded() {
    for bad in [
        similar("", "Sem identidade", None),
        similar(&"x".repeat(257), "Longa identidade", None),
        similar(OTHER, "", None),
        similar(OTHER, &"n".repeat(2049), None),
        similar(OTHER, "Controle\u{7}", None),
        similar(OTHER, "Negativa", Some(-0.1)),
        similar(OTHER, "Infinita", Some(f64::INFINITY)),
        similar(OTHER, "Indefinida", Some(f64::NAN)),
        similar(OTHER, "Enorme", Some(1.0e300)),
    ] {
        assert!(page(vec![bad]).validate_for(&request(5)).is_err());
    }
    page(vec![similar(OTHER, &"n".repeat(2048), Some(0.0))])
        .validate_for(&request(5))
        .unwrap();
}

#[test]
fn the_encoded_page_has_a_fixed_budget() {
    assert_eq!(MAX_SIMILAR_ARTISTS, 25);
    assert_eq!(MAX_SIMILAR_ARTISTS_BYTES, 32_768);
    let items = (0..MAX_SIMILAR_ARTISTS)
        .map(|index| similar(&format!("id-{index}"), &"n".repeat(2048), None))
        .collect();
    assert!(
        page(items)
            .validate_for(&request(MAX_SIMILAR_ARTISTS))
            .is_err(),
        "a page larger than the byte budget is rejected even within the row limit"
    );
}

#[test]
fn names_reject_invisible_and_bidirectional_formatting_but_keep_joiners() {
    for hidden in [
        "\u{202E}", "\u{202A}", "\u{2066}", "\u{2069}", "\u{200E}", "\u{200F}", "\u{061C}",
        "\u{200B}", "\u{2060}", "\u{FEFF}", "\u{2062}",
    ] {
        let name = format!("Banda{hidden}Falsa");
        assert!(
            page(vec![similar(OTHER, &name, None)])
                .validate_for(&request(5))
                .is_err(),
            "{hidden:?} must be rejected"
        );
    }
    for legitimate in ["می\u{200C}خواهم", "👩\u{200D}🎤 Artista", "Ünï Çödé"] {
        page(vec![similar(OTHER, legitimate, None)])
            .validate_for(&request(5))
            .unwrap();
    }
}
