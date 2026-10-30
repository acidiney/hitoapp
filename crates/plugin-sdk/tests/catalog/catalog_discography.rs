use hito_plugin_sdk::{ArtistDiscographyPage, ArtistDiscographyRequest};
use serde_json::json;

#[test]
fn discography_pages_preserve_group_identity_credits_and_the_requested_context() {
    let request: ArtistDiscographyRequest = serde_json::from_value(json!({
        "artist_id": "artist:owned/47~á",
        "cursor": null,
        "snapshot": null,
        "limit": 5
    }))
    .expect("the owned discography request must deserialize");
    request
        .validate()
        .expect("the first owned discography request must be valid");

    let wire = json!({
        "artist_id": "artist:owned/47~á",
        "snapshot": null,
        "items": [
            {
                "id": "group:owned/live",
                "title": "Owned Album",
                "artist_credit": [
                    {
                        "artist": {"id": "artist:owned/47~á", "name": "Owned Ensemble"},
                        "credited_name": "The Owned Ensemble",
                        "join_phrase": " & "
                    },
                    {
                        "artist": {"id": "artist:owned/47~á", "name": "Owned Ensemble"},
                        "credited_name": "Owned Ensemble",
                        "join_phrase": " feat. "
                    },
                    {
                        "artist": null,
                        "credited_name": "Owned Guest",
                        "join_phrase": ""
                    }
                ],
                "kind": "album",
                "secondary_types": ["live"],
                "first_release_date": "2024-05",
                "revision": null
            },
            {
                "id": "group:owned/remix",
                "title": "Owned Album",
                "artist_credit": null,
                "kind": "album",
                "secondary_types": ["remix"],
                "first_release_date": null,
                "revision": null
            }
        ],
        "total_count": null,
        "next_cursor": "cursor:owned/next"
    });
    let page: ArtistDiscographyPage =
        serde_json::from_value(wire.clone()).expect("the owned discography page must deserialize");
    page.validate_for(&request)
        .expect("a short live page with unknown total must be valid");
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].title, page.items[1].title);
    assert_ne!(page.items[0].id, page.items[1].id);
    assert!(page.snapshot.is_none());
    assert!(page.total_count.is_none());
    let credit = page.items[0]
        .artist_credit
        .as_ref()
        .expect("the first group must retain its observed credit");
    assert_eq!(credit.len(), 3);
    assert_eq!(credit[0].credited_name, "The Owned Ensemble");
    assert_eq!(credit[0].join_phrase, " & ");
    assert_eq!(credit[1].join_phrase, " feat. ");
    assert_eq!(
        credit[0].artist.as_ref().map(|artist| &artist.id),
        credit[1].artist.as_ref().map(|artist| &artist.id)
    );
    assert!(credit[2].artist.is_none());
    assert_eq!(credit[2].credited_name, "Owned Guest");
    assert_eq!(credit[2].join_phrase, "");
    assert_eq!(
        serde_json::to_value(&page).expect("the owned page must serialize"),
        wire
    );

    let another = ArtistDiscographyRequest {
        artist_id: "artist:owned/another".into(),
        cursor: None,
        snapshot: None,
        limit: 5,
    };
    assert!(
        page.validate_for(&another).is_err(),
        "a discography must belong to the requested provider artist"
    );

    let continuation = ArtistDiscographyRequest {
        artist_id: request.artist_id,
        cursor: Some("cursor:owned/previous".into()),
        snapshot: None,
        limit: 5,
    };
    page.validate_for(&continuation)
        .expect("a live continuation must retain its unknown snapshot");
    let changed_snapshot = ArtistDiscographyRequest {
        snapshot: Some("snapshot:owned/one".into()),
        ..continuation
    };
    assert!(
        page.validate_for(&changed_snapshot).is_err(),
        "a continuation must echo its requested snapshot"
    );
}
