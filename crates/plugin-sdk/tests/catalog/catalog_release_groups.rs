#[path = "../fixtures/catalog_discography.rs"]
mod fixture;

use hito_plugin_sdk::{ArtistDiscographyPage, MAX_CATALOG_CREDIT_MEMBERS};
use serde_json::{Value, json};

#[test]
fn release_groups_preserve_unknowns_and_known_primary_types_without_inventing_editions() {
    let request = fixture::request();
    let mut group = fixture::group("group:owned/one");
    group["artist_credit"] = Value::Null;
    for kind in [
        Value::Null,
        json!("album"),
        json!("ep"),
        json!("single"),
        json!("broadcast"),
        json!("other"),
    ] {
        group["kind"] = kind;
        let wire = fixture::page(vec![group.clone()]);
        assert!(fixture::accepted(wire.clone(), &request));
        let page: ArtistDiscographyPage =
            serde_json::from_value(wire.clone()).expect("the owned group must deserialize");
        assert!(page.items[0].artist_credit.is_none());
        assert!(page.items[0].first_release_date.is_none());
        assert!(page.items[0].revision.is_none());
        assert_eq!(
            serde_json::to_value(page).expect("the owned group must serialize"),
            wire
        );
    }
}

#[test]
fn bounded_credit_lists_allow_repeated_artists_and_unknown_members_but_reject_invalid_text() {
    assert_eq!(MAX_CATALOG_CREDIT_MEMBERS, 32);
    let request = fixture::request();
    let mut group = fixture::group("group:owned/one");
    let member = group["artist_credit"][0].clone();
    group["artist_credit"] = json!(vec![member.clone(); 32]);
    assert!(fixture::accepted(
        fixture::page(vec![group.clone()]),
        &request
    ));
    group["artist_credit"] = json!(vec![member.clone(); 33]);
    assert!(!fixture::accepted(
        fixture::page(vec![group.clone()]),
        &request
    ));
    group["artist_credit"] = json!([]);
    assert!(!fixture::accepted(
        fixture::page(vec![group.clone()]),
        &request
    ));

    group["artist_credit"] = json!([member]);
    group["artist_credit"][0]["artist"] = Value::Null;
    for join_phrase in ["", " ", " & "] {
        group["artist_credit"][0]["join_phrase"] = json!(join_phrase);
        assert!(fixture::accepted(
            fixture::page(vec![group.clone()]),
            &request
        ));
    }
    for (field, invalid) in [
        ("join_phrase", json!("\t")),
        ("join_phrase", json!("owned\njoin")),
        ("join_phrase", json!("x".repeat(129))),
        ("credited_name", json!(" ")),
        ("credited_name", json!("Owned\nname")),
        ("credited_name", json!(null)),
    ] {
        let mut malformed = group.clone();
        malformed["artist_credit"][0][field] = invalid;
        assert!(!fixture::accepted(fixture::page(vec![malformed]), &request));
    }
}

#[test]
fn release_group_and_credit_field_bounds_count_utf8_bytes() {
    let request = fixture::request();
    let mut group = fixture::group(&"é".repeat(128));
    group["title"] = json!("é".repeat(1024));
    group["revision"] = json!("é".repeat(128));
    group["artist_credit"][0]["artist"]["id"] = json!("é".repeat(128));
    group["artist_credit"][0]["artist"]["name"] = json!("é".repeat(1024));
    group["artist_credit"][0]["credited_name"] = json!("é".repeat(1024));
    group["artist_credit"][0]["join_phrase"] = json!("é".repeat(64));
    assert!(fixture::accepted(
        fixture::page(vec![group.clone()]),
        &request
    ));
    for path in [
        "/id",
        "/title",
        "/revision",
        "/artist_credit/0/artist/id",
        "/artist_credit/0/artist/name",
        "/artist_credit/0/credited_name",
        "/artist_credit/0/join_phrase",
    ] {
        let mut oversized = group.clone();
        let field = oversized
            .pointer_mut(path)
            .expect("the bounded field exists");
        let text = field.as_str().expect("the bounded field is text");
        *field = json!(format!("{text}é"));
        assert!(!fixture::accepted(fixture::page(vec![oversized]), &request));
    }

    let labels: Vec<String> = (0..16)
        .map(|index| format!("{index:02}{}", "é".repeat(63)))
        .collect();
    let mut secondary = fixture::group("group:owned/types");
    secondary["secondary_types"] = json!(labels);
    assert!(fixture::accepted(
        fixture::page(vec![secondary.clone()]),
        &request
    ));
    let mut too_many = labels.clone();
    too_many.push("owned seventeenth type".into());
    let mut duplicate = labels.clone();
    duplicate[15] = labels[0].clone();
    for invalid in [
        json!(too_many),
        json!(duplicate),
        json!(["é".repeat(65)]),
        json!([" "]),
        json!(["owned\ntype"]),
    ] {
        secondary["secondary_types"] = invalid;
        assert!(!fixture::accepted(
            fixture::page(vec![secondary.clone()]),
            &request
        ));
    }
}

#[test]
fn partial_release_dates_preserve_precision_and_require_valid_calendar_dates() {
    let request = fixture::request();
    let mut group = fixture::group("group:owned/date");
    for date in [
        "0001",
        "2024",
        "2024-02",
        "2024-02-29",
        "2000-02-29",
        "1900-02-28",
        "2024-04-30",
        "9999-12-31",
    ] {
        group["first_release_date"] = json!(date);
        let wire = fixture::page(vec![group.clone()]);
        assert!(fixture::accepted(wire.clone(), &request));
        let page: ArtistDiscographyPage =
            serde_json::from_value(wire).expect("the dated group must deserialize");
        assert_eq!(page.items[0].first_release_date.as_deref(), Some(date));
    }
    for invalid in [
        "",
        "0000",
        "202",
        "20240",
        "2024-2",
        "2024-00",
        "2024-13",
        "2024-02-30",
        "1900-02-29",
        "2100-02-29",
        "2023-02-29",
        "2024-04-31",
        "2024-01-00",
        "2024-01-32",
        "2024-01-1",
        "２０２４",
        "2024/02/29",
        "2024-02-29T00:00:00Z",
    ] {
        group["first_release_date"] = json!(invalid);
        assert!(!fixture::accepted(
            fixture::page(vec![group.clone()]),
            &request
        ));
    }
}

#[test]
fn nested_catalog_objects_reject_audio_urls_markup_and_malformed_required_fields() {
    let request = fixture::request();
    let page = fixture::page(vec![fixture::group("group:owned/one")]);
    for path in [
        "/url",
        "/items/0/audio_url",
        "/items/0/artist_credit/0/html",
        "/items/0/artist_credit/0/artist/provider_id",
    ] {
        let mut malformed = page.clone();
        let (parent, field) = path.rsplit_once('/').expect("the owned field has a parent");
        malformed
            .pointer_mut(parent)
            .expect("the owned parent exists")
            .as_object_mut()
            .expect("the owned parent is an object")
            .insert(field.into(), json!("owned unsupported field"));
        assert!(!fixture::accepted(malformed, &request));
    }
    for (path, invalid) in [
        ("/artist_id", json!("artist:owned/another")),
        ("/items/0/id", json!("")),
        ("/items/0/title", json!("Owned\nname")),
        ("/items/0/kind", json!("compilation")),
        ("/items/0/first_release_date", json!(2024)),
        ("/items/0/secondary_types", json!(null)),
        ("/items/0/artist_credit", json!("Owned Ensemble")),
        (
            "/items/0/artist_credit/0/artist",
            json!({"name": "Owned Ensemble"}),
        ),
    ] {
        let mut malformed = page.clone();
        *malformed.pointer_mut(path).expect("the owned field exists") = invalid;
        assert!(!fixture::accepted(malformed, &request));
    }
}
