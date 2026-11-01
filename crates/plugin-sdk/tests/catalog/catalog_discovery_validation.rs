use hito_plugin_sdk::{
    ArtistSearchCandidate, ArtistSearchPage, ArtistSearchRequest, CatalogArtistRef,
    CatalogDiscoveryCapabilities, MAX_ARTIST_SEARCH_PAGE_BYTES, MAX_ARTIST_SEARCH_PAGE_ITEMS,
    MAX_ARTIST_SEARCH_QUERY_BYTES, MAX_CATALOG_CURSOR_BYTES,
};
use serde_json::{Value, json};

fn request() -> ArtistSearchRequest {
    ArtistSearchRequest {
        query: "Owned Ensemble".into(),
        cursor: None,
        snapshot: None,
        limit: 20,
    }
}

fn candidate(id: &str) -> Value {
    json!({
        "artist": {"id": id, "name": "Owned Ensemble"},
        "kind": null,
        "disambiguation": null
    })
}

fn page(items: Vec<Value>) -> Value {
    json!({
        "query": "Owned Ensemble",
        "snapshot": null,
        "items": items,
        "total_count": null,
        "next_cursor": null
    })
}

fn accepted(wire: Value, request: &ArtistSearchRequest) -> bool {
    serde_json::from_value::<ArtistSearchPage>(wire)
        .is_ok_and(|page| page.validate_for(request).is_ok())
}

#[test]
fn discovery_descriptors_reject_unknown_versions_operations_and_fields() {
    for wire in [
        json!({"discovery_version": 0, "operations": ["artist_search"]}),
        json!({"discovery_version": 2, "operations": ["artist_search"]}),
        json!({"discovery_version": "1", "operations": ["artist_search"]}),
        json!({"discovery_version": 1, "operations": []}),
        json!({"discovery_version": 1, "operations": ["artist_profile"]}),
        json!({"discovery_version": 1, "operations": ["artist_search", "artist_search"]}),
        json!({"discovery_version": 1, "operations": ["artist_search", "artist_similarity"]}),
        json!({"discovery_version": 1, "operations": [null]}),
        json!({"discovery_version": 1, "operations": "artist_search"}),
        json!({"discovery_version": 1}),
        json!({"operations": ["artist_search"]}),
        json!({"discovery_version": 1, "operations": ["artist_search"], "html": "unsupported"}),
        Value::Null,
    ] {
        assert!(
            !serde_json::from_value::<CatalogDiscoveryCapabilities>(wire)
                .is_ok_and(|descriptor| descriptor.validate().is_ok()),
            "An undefined discovery declaration must not be accepted"
        );
    }
}

#[test]
fn search_request_bounds_count_utf8_bytes_and_reject_malformed_context() {
    assert_eq!(MAX_ARTIST_SEARCH_QUERY_BYTES, 512);
    assert_eq!(MAX_ARTIST_SEARCH_PAGE_ITEMS, 20);
    assert_eq!(MAX_CATALOG_CURSOR_BYTES, 2048);
    let bounded = ArtistSearchRequest {
        query: "é".repeat(256),
        cursor: Some("é".repeat(1024)),
        snapshot: Some("é".repeat(128)),
        limit: MAX_ARTIST_SEARCH_PAGE_ITEMS,
    };
    bounded
        .validate()
        .expect("The exact UTF-8 bounds must be accepted");
    let wire = serde_json::to_value(&bounded).unwrap();
    for field in ["query", "cursor", "snapshot"] {
        let mut oversized = wire.clone();
        let text = oversized[field].as_str().unwrap();
        oversized[field] = json!(format!("{text}é"));
        assert!(
            serde_json::from_value::<ArtistSearchRequest>(oversized)
                .unwrap()
                .validate()
                .is_err(),
            "An oversized UTF-8 context field must be rejected"
        );
        for invalid in ["", " ", "\u{a0}", "Owned\nvalue", "Owned\u{7f}value"] {
            let mut malformed = wire.clone();
            malformed[field] = json!(invalid);
            assert!(
                serde_json::from_value::<ArtistSearchRequest>(malformed)
                    .unwrap()
                    .validate()
                    .is_err(),
                "Present context must be nonblank and free of control characters"
            );
        }
    }
    for (field, invalid) in [
        ("limit", json!(0)),
        ("limit", json!(21)),
        ("limit", json!(-1)),
        ("limit", json!(65536)),
        ("limit", json!(1.5)),
        ("query", Value::Null),
        ("cursor", json!({"offset": 1})),
        ("snapshot", json!(7)),
        ("provider_id", json!("unsupported")),
    ] {
        let mut malformed = wire.clone();
        malformed[field] = invalid;
        assert!(
            !serde_json::from_value::<ArtistSearchRequest>(malformed)
                .is_ok_and(|request| request.validate().is_ok())
        );
    }
    let invalid_first = ArtistSearchRequest {
        snapshot: Some("owned-snapshot".into()),
        ..request()
    };
    assert!(invalid_first.validate().is_err());
    let smallest = ArtistSearchRequest {
        query: "é".into(),
        limit: 1,
        ..request()
    };
    smallest
        .validate()
        .expect("A single bounded character is a valid query");
}

#[test]
fn pages_keep_exact_query_snapshot_and_advancing_continuation_identity() {
    let first = request();
    let mut live = page(vec![candidate("artist:owned/one")]);
    live["next_cursor"] = json!("opaque:next/2");
    assert!(accepted(live.clone(), &first));
    let mut snapshot = live.clone();
    snapshot["snapshot"] = json!("snapshot:owned/one");
    assert!(accepted(snapshot.clone(), &first));
    let mut continuation = ArtistSearchRequest {
        cursor: Some("opaque:current/1".into()),
        ..request()
    };
    assert!(accepted(live.clone(), &continuation));
    assert!(!accepted(snapshot.clone(), &continuation));
    continuation.snapshot = Some("snapshot:owned/one".into());
    assert!(accepted(snapshot.clone(), &continuation));
    assert!(!accepted(live, &continuation));
    for different in ["Owned ensemble", "Owned Ensemble ", "Another Ensemble"] {
        let mut changed = snapshot.clone();
        changed["query"] = json!(different);
        assert!(!accepted(changed, &continuation));
    }
    for (field, value) in [
        ("snapshot", json!("snapshot:owned/another")),
        ("snapshot", json!("é".repeat(129))),
        ("next_cursor", json!("opaque:current/1")),
        ("next_cursor", json!("c".repeat(2049))),
        ("next_cursor", json!(" ")),
    ] {
        let mut changed = snapshot.clone();
        changed[field] = value;
        assert!(!accepted(changed, &continuation));
    }
    let mut empty = snapshot.clone();
    empty["items"] = json!([]);
    assert!(!accepted(empty.clone(), &continuation));
    empty["next_cursor"] = Value::Null;
    assert!(accepted(empty, &continuation));
    assert!(accepted(page(vec![]), &first));
    let mut maximum_cursor = snapshot;
    maximum_cursor["next_cursor"] = json!("é".repeat(1024));
    assert!(accepted(maximum_cursor, &continuation));
}

#[test]
fn candidate_identity_metadata_and_utf8_bounds_are_independent_of_display_name() {
    let request = request();
    let one = candidate("artist:owned/one?credit=lead");
    let two = candidate("artist:owned/two~á");
    assert!(accepted(page(vec![one.clone(), two]), &request));
    let mut duplicate = one.clone();
    duplicate["artist"]["name"] = json!("Different display name");
    assert!(!accepted(page(vec![one, duplicate]), &request));

    let mut bounded = candidate(&"é".repeat(128));
    bounded["artist"]["name"] = json!("é".repeat(1024));
    bounded["disambiguation"] = json!("é".repeat(1024));
    for kind in [
        "person",
        "group",
        "orchestra",
        "choir",
        "character",
        "other",
    ] {
        bounded["kind"] = json!(kind);
        assert!(accepted(page(vec![bounded.clone()]), &request));
    }
    for path in ["/artist/id", "/artist/name", "/disambiguation"] {
        let mut oversized = bounded.clone();
        let field = oversized.pointer_mut(path).unwrap();
        *field = json!(format!("{}é", field.as_str().unwrap()));
        assert!(!accepted(page(vec![oversized]), &request));
        for invalid in ["", " ", "Owned\tvalue", "Owned\u{7f}value"] {
            let mut malformed = bounded.clone();
            *malformed.pointer_mut(path).unwrap() = json!(invalid);
            assert!(!accepted(page(vec![malformed]), &request));
        }
    }
    for path in ["/audio_url", "/artist/provider_id", "/score", "/html"] {
        let mut malformed = bounded.clone();
        let (parent, field) = path.rsplit_once('/').unwrap();
        malformed
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!("unsupported"));
        assert!(!accepted(page(vec![malformed]), &request));
    }
    let mut invalid_kind = bounded;
    invalid_kind["kind"] = json!("unknown_kind");
    assert!(!accepted(page(vec![invalid_kind]), &request));
    let unknowns: ArtistSearchPage = serde_json::from_value(json!({
        "query": "Owned Ensemble", "items": [{"artist": {"id": "opaque:one", "name": "Owned Ensemble"}}]
    })).unwrap();
    unknowns.validate_for(&request).unwrap();
    assert!(unknowns.snapshot.is_none());
    assert!(unknowns.total_count.is_none());
    assert!(unknowns.next_cursor.is_none());
    assert!(unknowns.items[0].kind.is_none());
    assert!(unknowns.items[0].disambiguation.is_none());
    assert_eq!(
        serde_json::to_value(unknowns).unwrap(),
        page(vec![candidate("opaque:one")])
    );
}

#[test]
fn candidate_page_cardinality_and_totals_respect_the_requested_limit() {
    let request = request();
    let maximum = (0..20)
        .map(|index| candidate(&format!("opaque:{index}")))
        .collect();
    assert!(accepted(page(maximum), &request));
    let oversized = (0..21)
        .map(|index| candidate(&format!("opaque:{index}")))
        .collect();
    assert!(!accepted(page(oversized), &request));
    let one = ArtistSearchRequest {
        limit: 1,
        ..request
    };
    let mut single = page(vec![candidate("opaque:one")]);
    single["total_count"] = json!(1);
    assert!(accepted(single.clone(), &one));
    single["total_count"] = json!(u32::MAX);
    assert!(accepted(single.clone(), &one));
    for invalid in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(u64::from(u32::MAX) + 1),
    ] {
        single["total_count"] = invalid;
        assert!(!accepted(single.clone(), &one));
    }
    assert!(!accepted(
        page(vec![candidate("opaque:one"), candidate("opaque:two")]),
        &one
    ));
    for (field, invalid) in [
        ("items", Value::Null),
        ("query", Value::Null),
        ("url", json!("unsupported")),
    ] {
        let mut malformed = page(vec![]);
        malformed[field] = invalid;
        assert!(!accepted(malformed, &one));
    }
}

#[test]
fn escaped_candidate_pages_accept_exactly_64_kib_and_reject_one_more_valid_byte() {
    assert_eq!(MAX_ARTIST_SEARCH_PAGE_BYTES, 65_536);
    let request = request();
    let mut page = ArtistSearchPage {
        query: request.query.clone(),
        snapshot: None,
        items: (0..20)
            .map(|index| ArtistSearchCandidate {
                artist: CatalogArtistRef {
                    id: format!("opaque:{index}"),
                    name: "\"".repeat(600),
                },
                kind: None,
                disambiguation: Some("\\".repeat(600)),
                country: None,
                begin_year: None,
            })
            .collect(),
        total_count: None,
        next_cursor: None,
    };
    let initial = serde_json::to_vec(&page).unwrap().len();
    assert!(initial < MAX_ARTIST_SEARCH_PAGE_BYTES);
    let mut remaining = MAX_ARTIST_SEARCH_PAGE_BYTES - initial;
    for candidate in &mut page.items {
        let additional = remaining.min(2048 - candidate.artist.name.len());
        candidate.artist.name.push_str(&"x".repeat(additional));
        remaining -= additional;
    }
    assert_eq!(
        remaining, 0,
        "Valid bounded fields must accommodate the exact encoded budget"
    );
    for candidate in &page.items {
        candidate
            .validate()
            .expect("Each exact-budget candidate must remain field-valid");
    }
    assert_eq!(
        serde_json::to_vec(&page).unwrap().len(),
        MAX_ARTIST_SEARCH_PAGE_BYTES
    );
    page.validate_for(&request)
        .expect("The exact encoded page budget must remain valid");
    let last = page.items.last_mut().unwrap();
    assert!(last.artist.name.len() < 2048);
    last.artist.name.push('x');
    last.validate()
        .expect("The extra encoded byte must not violate an individual field bound");
    assert_eq!(
        serde_json::to_vec(&page).unwrap().len(),
        MAX_ARTIST_SEARCH_PAGE_BYTES + 1
    );
    assert!(page.validate_for(&request).is_err());
}
