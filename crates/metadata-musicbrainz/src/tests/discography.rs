use super::{
    ANOTHER_ARTIST_ID, ARTIST_ID,
    http::{Fixture, Reply},
};
use crate::MusicBrainzClient;
use hito_plugin_sdk::ArtistDiscographyRequest;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const FIRST_GROUP: &str = "0f710000-0000-4000-8000-000000000001";
const SECOND_GROUP: &str = "0f710000-0000-4000-8000-000000000002";
const LAST_GROUP: &str = "0f710000-0000-4000-8000-000000000003";

fn reply(offset: u32, groups: Value) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "release-group-count": 3,
            "release-group-offset": offset,
            "release-groups": groups
        }))
        .expect("the owned browse reply must serialize"),
    )
}

fn request(cursor: Option<String>) -> ArtistDiscographyRequest {
    ArtistDiscographyRequest {
        artist_id: ARTIST_ID.into(),
        cursor,
        snapshot: None,
        limit: 50,
    }
}

#[tokio::test]
async fn discography_pages_preserve_group_identity_credits_and_honest_unknowns() {
    let fixture = Fixture::start(vec![
        reply(
            0,
            json!([
                {
                    "id": FIRST_GROUP,
                    "title": "Owned shared title",
                    "primary-type": "Album",
                    "secondary-types": ["Live"],
                    "first-release-date": "2020-02",
                    "artist-credit": [
                        {
                            "name": "Owned credited alias",
                            "joinphrase": " & ",
                            "artist": {"id": ARTIST_ID, "name": "Owned canonical performer"}
                        },
                        {
                            "name": "Owned guest alias",
                            "joinphrase": "",
                            "artist": {"id": ANOTHER_ARTIST_ID, "name": "Owned canonical guest"}
                        }
                    ]
                },
                {
                    "id": SECOND_GROUP,
                    "title": "Owned shared title",
                    "primary-type": null,
                    "secondary-types": [],
                    "first-release-date": ""
                }
            ]),
        ),
        reply(
            2,
            json!([{
                "id": LAST_GROUP,
                "title": "Owned final concept",
                "primary-type": "EP",
                "secondary-types": [],
                "first-release-date": "2021",
                "artist-credit": null
            }]),
        ),
    ])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned discography client must initialize");

    let result = async {
        let first = client.artist_discography(request(None)).await?;
        let cursor = first.next_cursor.clone();
        let last = client.artist_discography(request(cursor.clone())).await?;
        Ok::<_, crate::Error>((first, cursor, last))
    }
    .await;
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    let (first, cursor, last) = result.expect("both owned discography pages must load");

    assert!(cursor.as_ref().is_some_and(|cursor| !cursor.is_empty()));
    assert_eq!(requests.len(), 2, "one request must load exactly one page");
    for (uri, offset) in requests.iter().zip(["0", "2"]) {
        assert_eq!(uri.path(), "/ws/2/release-group");
        let absolute = endpoint
            .join(&uri.to_string())
            .expect("the observed request must be a URL");
        let query = absolute
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            query,
            BTreeMap::from([
                ("artist".into(), ARTIST_ID.into()),
                ("fmt".into(), "json".into()),
                ("inc".into(), "artist-credits".into()),
                ("limit".into(), "50".into()),
                ("offset".into(), offset.into()),
            ])
        );
    }
    first
        .validate_for(&request(None))
        .expect("the first page must honor the shared catalog contract");
    last.validate_for(&request(cursor.clone()))
        .expect("the continuation must honor its request context");
    assert_eq!(
        serde_json::to_value(first).expect("the first page must serialize"),
        json!({
            "artist_id": ARTIST_ID,
            "snapshot": null,
            "items": [
                {
                    "id": FIRST_GROUP,
                    "title": "Owned shared title",
                    "artist_credit": [
                        {
                            "artist": {"id": ARTIST_ID, "name": "Owned canonical performer"},
                            "credited_name": "Owned credited alias",
                            "join_phrase": " & "
                        },
                        {
                            "artist": {"id": ANOTHER_ARTIST_ID, "name": "Owned canonical guest"},
                            "credited_name": "Owned guest alias",
                            "join_phrase": ""
                        }
                    ],
                    "kind": "album",
                    "secondary_types": ["Live"],
                    "first_release_date": "2020-02",
                    "revision": null
                },
                {
                    "id": SECOND_GROUP,
                    "title": "Owned shared title",
                    "artist_credit": null,
                    "kind": null,
                    "secondary_types": [],
                    "first_release_date": null,
                    "revision": null
                }
            ],
            "total_count": 3,
            "next_cursor": cursor
        })
    );
    assert_eq!(
        serde_json::to_value(last).expect("the last page must serialize"),
        json!({
            "artist_id": ARTIST_ID,
            "snapshot": null,
            "items": [{
                "id": LAST_GROUP,
                "title": "Owned final concept",
                "artist_credit": null,
                "kind": "ep",
                "secondary_types": [],
                "first_release_date": "2021",
                "revision": null
            }],
            "total_count": 3,
            "next_cursor": null
        })
    );
}
