use super::{
    ARTIST_ID,
    http::{Fixture, Reply},
};
use crate::MusicBrainzClient;
use hito_plugin_sdk::{EditionTrackListRequest, ReleaseGroupEditionsRequest};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const GROUP_ID: &str = "0f710000-0000-4000-8000-000000000001";
const STANDARD_ID: &str = "0f720000-0000-4000-8000-000000000001";
const BONUS_ID: &str = "0f720000-0000-4000-8000-000000000002";
const MAIN_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000001";
const BONUS_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000002";

fn request(cursor: Option<String>) -> ReleaseGroupEditionsRequest {
    ReleaseGroupEditionsRequest {
        release_group_id: GROUP_ID.into(),
        cursor,
        snapshot: None,
        limit: 2,
    }
}

fn browse_reply(offset: u32, edition: Value) -> Reply {
    Reply::json(
        serde_json::to_vec(&json!({
            "release-count": 2,
            "release-offset": offset,
            "releases": [edition]
        }))
        .expect("the owned edition page must serialize"),
    )
}

#[tokio::test]
async fn release_group_editions_page_by_identity_before_explicit_track_list_selection() {
    let credit = json!([{
        "name": "Owned credited performer",
        "joinphrase": "",
        "artist": {"id": ARTIST_ID, "name": "Owned canonical performer"}
    }]);
    let fixture = Fixture::start(vec![
        browse_reply(
            0,
            json!({
                "id": STANDARD_ID,
                "title": "Owned shared album title",
                "artist-credit": credit,
                "disambiguation": "standard edition",
                "date": "2020-02",
                "country": "GB",
                "release-group": {"id": GROUP_ID, "title": "Owned album concept"}
            }),
        ),
        browse_reply(
            1,
            json!({
                "id": BONUS_ID,
                "title": "Owned shared album title",
                "artist-credit": null,
                "disambiguation": "bonus disc",
                "date": "",
                "country": null,
                "release-group": {"id": GROUP_ID, "title": "Owned album concept"}
            }),
        ),
        Reply::json(
            serde_json::to_vec(&json!({
                "id": BONUS_ID,
                "title": "Owned shared album title",
                "artist-credit": null,
                "release-group": {"id": GROUP_ID, "title": "Owned album concept"},
                "media": [{
                    "position": 1,
                    "title": "",
                    "format": null,
                    "track-count": 2,
                    "track-offset": 0,
                    "tracks": [
                        {
                            "id": MAIN_TRACK_ID, "position": 1, "number": "1",
                            "title": "Owned main track", "length": null,
                            "artist-credit": null
                        },
                        {
                            "id": BONUS_TRACK_ID, "position": 2, "number": "Bonus",
                            "title": "Owned bonus track", "length": null,
                            "artist-credit": null
                        }
                    ]
                }]
            }))
            .expect("the owned selected edition must serialize"),
        ),
    ])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned edition-list client must initialize");

    let result = async {
        let first = client.release_group_editions(request(None)).await?;
        let cursor = first.next_cursor.clone();
        let last = client
            .release_group_editions(request(cursor.clone()))
            .await?;
        let before_selection = fixture.request_count();
        let selected_id = last
            .items
            .iter()
            .find(|edition| edition.id == BONUS_ID)
            .map(|edition| edition.id.clone())
            .unwrap_or_default();
        let selected = client
            .edition_track_list(EditionTrackListRequest {
                edition_id: selected_id,
                release_group_id: GROUP_ID.into(),
            })
            .await?;
        Ok::<_, crate::Error>((first, cursor, last, before_selection, selected))
    }
    .await;
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    let (first, cursor, last, before_selection, selected) =
        result.expect("edition pages and the explicitly selected track list must load");

    assert!(cursor.as_ref().is_some_and(|cursor| !cursor.is_empty()));
    assert_eq!(
        before_selection, 2,
        "listing must not fetch any edition track list"
    );
    assert_eq!(requests.len(), 3);
    for (uri, offset) in requests[..2].iter().zip(["0", "1"]) {
        assert_eq!(uri.path(), "/ws/2/release");
        let observed = endpoint
            .join(&uri.to_string())
            .expect("the observed edition-page request must be a URL");
        assert_eq!(observed.origin(), endpoint.origin());
        assert_eq!(
            observed
                .query_pairs()
                .into_owned()
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from([
                ("release-group".into(), GROUP_ID.into()),
                ("fmt".into(), "json".into()),
                ("inc".into(), "artist-credits release-groups".into()),
                ("limit".into(), "2".into()),
                ("offset".into(), offset.into()),
            ])
        );
    }
    assert_eq!(requests[2].path(), format!("/ws/2/release/{BONUS_ID}"));
    let lookup = endpoint
        .join(&requests[2].to_string())
        .expect("the explicit edition lookup must be a URL");
    assert_eq!(lookup.origin(), endpoint.origin());
    assert_eq!(
        lookup
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("fmt".into(), "json".into()),
            (
                "inc".into(),
                "recordings artist-credits release-groups".into()
            ),
        ])
    );
    first
        .validate_for(&request(None))
        .expect("the first edition page must preserve the requested group");
    last.validate_for(&request(cursor.clone()))
        .expect("the edition continuation must preserve its group and snapshot");
    assert_eq!(
        serde_json::to_value(first).expect("the first page must serialize"),
        json!({
            "release_group_id": GROUP_ID,
            "snapshot": null,
            "items": [{
                "id": STANDARD_ID,
                "title": "Owned shared album title",
                "artist_credit": [{
                    "artist": {"id": ARTIST_ID, "name": "Owned canonical performer"},
                    "credited_name": "Owned credited performer", "join_phrase": ""
                }],
                "disambiguation": "standard edition",
                "release_date": "2020-02",
                "country": "GB",
                "revision": null
            }],
            "total_count": 2,
            "next_cursor": cursor
        })
    );
    assert_eq!(
        serde_json::to_value(last).expect("the final page must serialize"),
        json!({
            "release_group_id": GROUP_ID,
            "snapshot": null,
            "items": [{
                "id": BONUS_ID,
                "title": "Owned shared album title",
                "artist_credit": null,
                "disambiguation": "bonus disc",
                "release_date": null,
                "country": null,
                "revision": null
            }],
            "total_count": 2,
            "next_cursor": null
        })
    );
    assert_eq!(selected.id, BONUS_ID);
    assert_eq!(selected.release_group_id, GROUP_ID);
    assert_eq!(selected.media.len(), 1);
    assert_eq!(
        selected.media[0]
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        vec![MAIN_TRACK_ID, BONUS_TRACK_ID]
    );
}
