#[path = "../fixtures/catalog_discography.rs"]
mod fixture;

use hito_plugin_sdk::{ArtistDiscographyRequest, MAX_CATALOG_PAGE_ITEMS};
use serde_json::{Value, json};

#[test]
fn first_pages_choose_live_or_snapshot_results_and_continuations_keep_that_context() {
    let first = fixture::request();
    let mut live = fixture::page(vec![fixture::group("group:owned/one")]);
    live["next_cursor"] = json!("cursor:owned/next");
    assert!(fixture::accepted(live.clone(), &first));

    let mut snapshot = live.clone();
    snapshot["snapshot"] = json!("snapshot:owned/one");
    assert!(fixture::accepted(snapshot.clone(), &first));

    let mut continuation = fixture::request();
    continuation.cursor = Some("cursor:owned/previous".into());
    assert!(fixture::accepted(live.clone(), &continuation));
    assert!(!fixture::accepted(snapshot.clone(), &continuation));
    continuation.snapshot = Some("snapshot:owned/one".into());
    assert!(fixture::accepted(snapshot.clone(), &continuation));
    assert!(!fixture::accepted(live, &continuation));
    snapshot["snapshot"] = json!("snapshot:owned/another");
    assert!(!fixture::accepted(snapshot, &continuation));

    let mut invalid_first = fixture::request();
    invalid_first.snapshot = Some("snapshot:owned/unrequested".into());
    assert!(invalid_first.validate().is_err());
    assert!(!fixture::accepted(
        fixture::page(vec![fixture::group("group:owned/one")]),
        &invalid_first
    ));

    let mut final_page = fixture::page(vec![]);
    final_page["snapshot"] = json!("snapshot:owned/one");
    final_page["total_count"] = json!(1000);
    assert!(fixture::accepted(final_page, &continuation));
    assert!(fixture::accepted(fixture::page(vec![]), &first));
}

#[test]
fn pages_reject_duplicate_groups_and_continuations_without_progress() {
    let mut request = fixture::request();
    request.cursor = Some("cursor:owned/current".into());
    let group = fixture::group("group:owned/one");
    let mut page = fixture::page(vec![group.clone()]);
    page["next_cursor"] = json!("cursor:owned/next");
    assert!(fixture::accepted(page.clone(), &request));

    let mut duplicate = page.clone();
    duplicate["items"] = json!([group.clone(), group]);
    assert!(!fixture::accepted(duplicate, &request));

    let mut unchanged_cursor = page.clone();
    unchanged_cursor["next_cursor"] = json!("cursor:owned/current");
    assert!(!fixture::accepted(unchanged_cursor, &request));
    page["items"] = json!([]);
    assert!(!fixture::accepted(page, &request));
}

#[test]
fn request_limits_and_page_counts_are_checked_without_guessing_cursor_offsets() {
    assert_eq!(MAX_CATALOG_PAGE_ITEMS, 50);
    let request = fixture::request();
    let wire = serde_json::to_value(&request).expect("the owned request must serialize");
    for (field, invalid) in [
        ("limit", json!(0)),
        ("limit", json!(51)),
        ("limit", json!(-1)),
        ("limit", json!(65536)),
        ("limit", json!(1.5)),
        ("artist_id", json!(null)),
        ("cursor", json!({"offset": 5})),
        ("url", json!("owned unsupported field")),
    ] {
        let mut malformed = wire.clone();
        malformed[field] = invalid;
        assert!(
            !serde_json::from_value::<ArtistDiscographyRequest>(malformed)
                .is_ok_and(|request| request.validate().is_ok())
        );
    }

    let mut one = fixture::request();
    one.limit = 1;
    let group = fixture::group("group:owned/one");
    let mut page = fixture::page(vec![group.clone()]);
    page["total_count"] = json!(1);
    assert!(fixture::accepted(page.clone(), &one));
    page["total_count"] = json!(0);
    assert!(!fixture::accepted(page, &one));
    assert!(!fixture::accepted(
        fixture::page(vec![group, fixture::group("group:owned/two")]),
        &one
    ));

    let mut maximum = fixture::request();
    maximum.limit = 50;
    let groups = (0..50)
        .map(|index| fixture::group(&format!("group:owned/{index}")))
        .collect();
    assert!(fixture::accepted(fixture::page(groups), &maximum));
}

#[test]
fn cursor_snapshot_and_artist_bounds_use_utf8_bytes() {
    let mut request = fixture::request();
    request.artist_id = "é".repeat(128);
    request.cursor = Some("é".repeat(1024));
    request.snapshot = Some("é".repeat(128));
    request
        .validate()
        .expect("bounded UTF-8 context must be valid");
    let mut page = fixture::page(vec![fixture::group("group:owned/one")]);
    page["artist_id"] = json!(request.artist_id);
    page["snapshot"] = json!(request.snapshot);
    page["next_cursor"] = json!("n".repeat(2048));
    assert!(fixture::accepted(page.clone(), &request));
    page["next_cursor"] = json!("n".repeat(2049));
    assert!(!fixture::accepted(page, &request));

    let wire = serde_json::to_value(&request).expect("the bounded request must serialize");
    for field in ["artist_id", "cursor", "snapshot"] {
        let mut oversized = wire.clone();
        let text = oversized[field]
            .as_str()
            .expect("the context field is text");
        oversized[field] = json!(format!("{text}é"));
        let request: ArtistDiscographyRequest =
            serde_json::from_value(oversized).expect("oversized text must deserialize");
        assert!(request.validate().is_err());
    }
    for (field, invalid) in [
        ("artist_id", json!("")),
        ("cursor", json!(" ")),
        ("snapshot", json!("owned\nvalue")),
    ] {
        let mut malformed = wire.clone();
        malformed[field] = invalid;
        let request: ArtistDiscographyRequest =
            serde_json::from_value(malformed).expect("malformed text must deserialize");
        assert!(request.validate().is_err());
    }

    let mut first_page = fixture::page(vec![fixture::group("group:owned/one")]);
    first_page["snapshot"] = json!("é".repeat(128));
    assert!(fixture::accepted(first_page.clone(), &fixture::request()));
    first_page["snapshot"] = json!("é".repeat(129));
    assert!(!fixture::accepted(first_page, &fixture::request()));
    assert!(!fixture::accepted(Value::Null, &fixture::request()));
}
