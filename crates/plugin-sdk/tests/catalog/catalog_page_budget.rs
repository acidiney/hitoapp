#[path = "../fixtures/catalog_discography.rs"]
mod fixture;

use hito_plugin_sdk::{ArtistDiscographyPage, MAX_CATALOG_PAGE_BYTES};
use serde_json::{Value, json};

#[test]
fn complete_encoded_pages_accept_the_exact_budget_and_reject_one_more_valid_byte() {
    assert_eq!(MAX_CATALOG_PAGE_BYTES, 262_144);
    let request = fixture::request();
    let member = json!({
        "artist": null,
        "credited_name": format!("Owned {}", "\"".repeat(900)),
        "join_phrase": ""
    });
    let groups = (0..4)
        .map(|index| {
            let mut group = fixture::group(&format!("group:owned/{index}"));
            group["artist_credit"] = json!(vec![member.clone(); 32]);
            group
        })
        .collect();
    let mut wire = fixture::page(groups);
    let initial_length = serde_json::to_vec(&wire)
        .expect("the owned budget fixture must serialize")
        .len();
    assert!(initial_length < MAX_CATALOG_PAGE_BYTES);
    let mut remaining = MAX_CATALOG_PAGE_BYTES - initial_length;
    for group in wire["items"].as_array_mut().expect("owned groups exist") {
        for member in group["artist_credit"]
            .as_array_mut()
            .expect("owned credits exist")
        {
            let name = member["credited_name"]
                .as_str()
                .expect("the owned credit is text");
            let additional = remaining.min(2048 - name.len());
            member["credited_name"] = json!(format!("{name}{}", "x".repeat(additional)));
            remaining -= additional;
        }
    }
    assert_eq!(
        remaining, 0,
        "the valid fields must accommodate the exact budget"
    );
    let page: ArtistDiscographyPage =
        serde_json::from_value(wire.clone()).expect("the exact-budget page must deserialize");
    for group in &page.items {
        group.validate().expect("each complete group must be valid");
    }
    assert_eq!(
        serde_json::to_vec(&page)
            .expect("the exact-budget page must serialize")
            .len(),
        MAX_CATALOG_PAGE_BYTES
    );
    assert!(fixture::accepted(wire.clone(), &request));

    let field = wire
        .pointer_mut("/items/3/artist_credit/31/credited_name")
        .expect("the final owned credit exists");
    let name = field.as_str().expect("the final owned credit is text");
    assert!(name.len() < 2048);
    *field = json!(format!("{name}x"));
    let oversized: ArtistDiscographyPage = serde_json::from_value(wire.clone())
        .expect("the oversized page must remain structurally valid");
    for group in &oversized.items {
        group
            .validate()
            .expect("overflow must not depend on an invalid field or group");
    }
    assert_eq!(
        serde_json::to_vec(&oversized)
            .expect("the oversized page must serialize")
            .len(),
        MAX_CATALOG_PAGE_BYTES + 1
    );
    assert!(!fixture::accepted(wire, &request));
    assert!(oversized.validate_for(&request).is_err());
    assert!(serde_json::from_value::<ArtistDiscographyPage>(Value::Null).is_err());
}
