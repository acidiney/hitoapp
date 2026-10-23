use hito_plugin_sdk::{ArtistDiscographyPage, ArtistDiscographyRequest};
use serde_json::{Value, json};

pub fn request() -> ArtistDiscographyRequest {
    ArtistDiscographyRequest {
        artist_id: "artist:owned/47".into(),
        cursor: None,
        snapshot: None,
        limit: 5,
    }
}

pub fn group(id: &str) -> Value {
    json!({
        "id": id,
        "title": "Owned album",
        "artist_credit": [{
            "artist": {"id": "artist:owned/47", "name": "Owned Ensemble"},
            "credited_name": "The Owned Ensemble",
            "join_phrase": ""
        }],
        "kind": null,
        "secondary_types": [],
        "first_release_date": null,
        "revision": null
    })
}

pub fn page(groups: Vec<Value>) -> Value {
    json!({
        "artist_id": "artist:owned/47",
        "snapshot": null,
        "items": groups,
        "total_count": null,
        "next_cursor": null
    })
}

pub fn accepted(wire: Value, request: &ArtistDiscographyRequest) -> bool {
    serde_json::from_value::<ArtistDiscographyPage>(wire)
        .is_ok_and(|page| page.validate_for(request).is_ok())
}
