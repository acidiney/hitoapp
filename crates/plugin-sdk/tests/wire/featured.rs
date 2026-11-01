use hito_plugin_sdk::{
    ArtworkEntity, ArtworkRequest, FEATURED_PATH, FeaturedItem, FeaturedKind, FeaturedPage,
    FeaturedRequest, MAX_FEATURED_BYTES, MAX_FEATURED_ITEMS, Manifest,
};
use serde_json::{Value, json};

fn item(id: &str) -> Value {
    json!({
        "id": id,
        "title": format!("Title {id}"),
        "subtitle": "Subtitle",
        "kind": "playlist",
        "reference": format!("https://provider.example/playlist/{id}"),
        "artwork_id": format!("art-{id}"),
        "item_count": 12
    })
}

fn page(items: Vec<Value>) -> FeaturedPage {
    serde_json::from_value(json!({"items": items})).unwrap()
}

#[test]
fn featured_requests_and_pages_keep_their_exact_wire_shape() {
    assert_eq!(FEATURED_PATH, "v1/featured");
    assert_eq!(MAX_FEATURED_ITEMS, 24);
    let request: FeaturedRequest = serde_json::from_value(json!({"limit": 12})).unwrap();
    request.validate().unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), json!({"limit": 12}));

    for (wire, kind) in [
        ("playlist", FeaturedKind::Playlist),
        ("album", FeaturedKind::Album),
        ("new_release", FeaturedKind::NewRelease),
    ] {
        let mut value = item("one");
        value["kind"] = json!(wire);
        let parsed: FeaturedItem = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed.kind, kind);
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    let minimal = json!({
        "id": "one", "title": "One", "subtitle": null, "kind": "album",
        "reference": "ref", "artwork_id": null, "item_count": null
    });
    let parsed: FeaturedItem = serde_json::from_value(minimal.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), minimal);

    let full = page(vec![item("a"), item("b")]);
    full.validate_for(&request).unwrap();
    assert_eq!(
        serde_json::to_value(&full).unwrap(),
        json!({"items": [item("a"), item("b")]})
    );
    page(vec![]).validate_for(&request).unwrap();
}

#[test]
fn featured_requests_are_bounded_and_closed() {
    for limit in [0_u16, 25, u16::MAX] {
        let request = FeaturedRequest { limit };
        assert_eq!(
            request.validate().unwrap_err(),
            "Use a featured limit between one and 24"
        );
    }
    for limit in [1_u16, 24] {
        FeaturedRequest { limit }.validate().unwrap();
    }
    assert!(
        serde_json::from_value::<FeaturedRequest>(json!({"limit": 5, "account": "x"})).is_err()
    );
    assert!(serde_json::from_value::<FeaturedRequest>(json!({})).is_err());
    assert!(serde_json::from_value::<FeaturedRequest>(json!({"limit": -1})).is_err());
}

#[test]
fn featured_pages_reject_unbounded_duplicate_or_unsafe_items() {
    let request = FeaturedRequest { limit: 3 };
    assert_eq!(
        page(vec![item("a"), item("b"), item("c"), item("d")])
            .validate_for(&request)
            .unwrap_err(),
        "The provider returned too many featured items"
    );
    assert_eq!(
        page(vec![item("a"), item("a")])
            .validate_for(&request)
            .unwrap_err(),
        "The provider repeated a featured item identity"
    );

    let rejected = |mutate: &dyn Fn(&mut Value)| {
        let mut value = item("a");
        mutate(&mut value);
        page(vec![value]).validate_for(&request).is_err()
    };
    assert!(rejected(&|v| v["id"] = json!("")));
    assert!(rejected(&|v| v["id"] = json!("a".repeat(257))));
    assert!(rejected(&|v| v["title"] = json!("   ")));
    assert!(rejected(&|v| v["title"] = json!("bad\ntitle")));
    assert!(rejected(&|v| v["title"] = json!("t".repeat(257))));
    assert!(rejected(&|v| v["subtitle"] = json!("bad\u{7f}")));
    assert!(rejected(&|v| v["subtitle"] = json!("s".repeat(257))));
    assert!(rejected(&|v| v["reference"] = json!("")));
    assert!(rejected(&|v| v["reference"] = json!("r".repeat(2049))));
    assert!(rejected(&|v| v["reference"] = json!("ref\u{0}")));
    assert!(rejected(&|v| v["artwork_id"] = json!("")));
    assert!(rejected(&|v| v["artwork_id"] = json!("a".repeat(257))));
    assert!(rejected(&|v| v["item_count"] = json!(10_001)));
    assert!(!rejected(&|v| v["item_count"] = json!(10_000)));
    assert!(!rejected(&|v| v["item_count"] = json!(0)));

    for field in ["url", "image", "audio"] {
        let mut value = item("a");
        value[field] = json!("https://provider.example/x");
        assert!(serde_json::from_value::<FeaturedItem>(value).is_err());
    }
    assert!(
        serde_json::from_value::<FeaturedPage>(json!({"items": [], "next_cursor": "x"})).is_err()
    );
    let mut unknown = item("a");
    unknown["kind"] = json!("chart");
    assert!(serde_json::from_value::<FeaturedItem>(unknown).is_err());
}

#[test]
fn featured_pages_stop_at_the_encoded_size_limit() {
    assert_eq!(MAX_FEATURED_BYTES, 32_768);
    let request = FeaturedRequest { limit: 24 };
    let wide = |index: usize| {
        let mut value = item(&format!("{index:03}"));
        value["title"] = json!("t".repeat(256));
        value["subtitle"] = json!("s".repeat(256));
        value["reference"] = json!("r".repeat(2048));
        value
    };
    let oversized = page((0..24).map(wide).collect());
    assert_eq!(
        oversized.validate_for(&request).unwrap_err(),
        "The provider featured items exceed the size limit"
    );
    page((0..10).map(wide).collect())
        .validate_for(&request)
        .unwrap();
}

#[test]
fn featured_is_a_manifest_capability_that_needs_playlists() {
    let declared = |capabilities: Value| {
        serde_json::from_value::<Manifest>(json!({
            "id": "org.hito.featured", "name": "Featured", "protocol_version": 1,
            "capabilities": capabilities
        }))
    };
    let both = declared(json!(["playlists", "featured"])).unwrap();
    both.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&both).unwrap()["capabilities"],
        json!(["playlists", "featured"])
    );
    assert_eq!(
        declared(json!(["featured"]))
            .unwrap()
            .validate()
            .unwrap_err(),
        "Featured items need the playlists capability to open"
    );
    declared(json!([
        "search",
        "sources",
        "playlists",
        "artwork",
        "featured"
    ]))
    .unwrap()
    .validate()
    .unwrap();
}

#[test]
fn featured_artwork_is_a_typed_artwork_entity() {
    let value = json!({"entity": "featured", "provider_id": "art-one"});
    let request: ArtworkRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(request.entity, ArtworkEntity::Featured);
    request.validate().unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), value);
}
