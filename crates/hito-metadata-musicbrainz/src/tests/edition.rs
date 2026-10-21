use super::{
    ARTIST_ID,
    http::{Fixture, Reply},
};
use crate::MusicBrainzClient;
use hito_plugin_sdk::EditionTrackListRequest;
use serde_json::json;
use std::collections::BTreeMap;

const EDITION_ID: &str = "0f720000-0000-4000-8000-000000000001";
const GROUP_ID: &str = "0f710000-0000-4000-8000-000000000001";
const PREGAP_ID: &str = "0f730000-0000-4000-8000-000000000001";
const FIRST_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000002";
const SECOND_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000003";
const DATA_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000004";
const REPEATED_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000005";
const BONUS_TRACK_ID: &str = "0f730000-0000-4000-8000-000000000006";
const REPEATED_RECORDING_ID: &str = "0f740000-0000-4000-8000-000000000001";
const SECOND_RECORDING_ID: &str = "0f740000-0000-4000-8000-000000000002";
const PREGAP_RECORDING_ID: &str = "0f740000-0000-4000-8000-000000000003";
const DATA_RECORDING_ID: &str = "0f740000-0000-4000-8000-000000000004";
const BONUS_RECORDING_ID: &str = "0f740000-0000-4000-8000-000000000005";

#[tokio::test]
async fn selected_edition_preserves_complete_order_and_occurrence_identity() {
    let artist_credit = json!([{
        "name": "Owned credited ensemble",
        "joinphrase": "",
        "artist": {"id": ARTIST_ID, "name": "Owned canonical ensemble"}
    }]);
    let reply = json!({
        "id": EDITION_ID,
        "title": "Owned bonus edition",
        "artist-credit": artist_credit,
        "release-group": {"id": GROUP_ID, "title": "Owned album concept"},
        "media": [
            {
                "position": 1,
                "title": "Owned main disc",
                "format": "Enhanced CD",
                "track-count": 2,
                "track-offset": 0,
                "pregap": {
                    "id": PREGAP_ID, "position": 0, "number": "0",
                    "title": "Owned hidden introduction", "length": 5000,
                    "artist-credit": null,
                    "recording": {"id": PREGAP_RECORDING_ID, "title": "Owned introduction recording"}
                },
                "tracks": [
                    {
                        "id": FIRST_TRACK_ID, "position": 1, "number": "A1",
                        "title": "Owned first occurrence", "length": 120000,
                        "artist-credit": artist_credit,
                        "recording": {"id": REPEATED_RECORDING_ID, "title": "Owned shared recording"}
                    },
                    {
                        "id": SECOND_TRACK_ID, "position": 2, "number": "A2",
                        "title": "Owned unknown-duration track", "length": null,
                        "artist-credit": null,
                        "recording": {"id": SECOND_RECORDING_ID, "title": "Owned second recording"}
                    }
                ],
                "data-tracks": [{
                    "id": DATA_TRACK_ID, "position": 3, "number": "3",
                    "title": "Owned data session", "length": null,
                    "artist-credit": null,
                    "recording": {"id": DATA_RECORDING_ID, "title": "Owned data recording"}
                }]
            },
            {
                "position": 2,
                "title": "",
                "format": null,
                "track-count": 2,
                "track-offset": 0,
                "tracks": [
                    {
                        "id": REPEATED_TRACK_ID, "position": 1, "number": "B1",
                        "title": "Owned repeated occurrence", "length": 120000,
                        "artist-credit": null,
                        "recording": {"id": REPEATED_RECORDING_ID, "title": "Owned shared recording"}
                    },
                    {
                        "id": BONUS_TRACK_ID, "position": 2, "number": "Bonus",
                        "title": "Owned bonus track", "length": 90000,
                        "artist-credit": null,
                        "recording": {"id": BONUS_RECORDING_ID, "title": "Owned bonus recording"}
                    }
                ]
            }
        ]
    });
    let fixture = Fixture::start(vec![Reply::json(
        serde_json::to_vec(&reply).expect("the owned edition reply must serialize"),
    )])
    .await;
    let client = MusicBrainzClient::with_test_endpoint(fixture.endpoint.clone())
        .expect("the owned edition client must initialize");

    let result = client
        .edition_track_list(EditionTrackListRequest {
            edition_id: EDITION_ID.into(),
            release_group_id: GROUP_ID.into(),
        })
        .await;
    drop(client);
    let requests = fixture.requests();
    let endpoint = fixture.endpoint.clone();
    fixture.close().await;
    let edition = result.expect("the exact selected edition must return its complete track list");

    assert_eq!(requests.len(), 1, "one exact lookup must read one edition");
    assert_eq!(requests[0].path(), format!("/ws/2/release/{EDITION_ID}"));
    let observed = endpoint
        .join(&requests[0].to_string())
        .expect("the observed edition lookup must be a URL");
    assert_eq!(observed.origin(), endpoint.origin());
    let query = observed
        .query_pairs()
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        query,
        BTreeMap::from([
            ("fmt".into(), "json".into()),
            (
                "inc".into(),
                "recordings artist-credits release-groups".into()
            ),
        ])
    );
    let credit = json!([{
        "artist": {"id": ARTIST_ID, "name": "Owned canonical ensemble"},
        "credited_name": "Owned credited ensemble",
        "join_phrase": ""
    }]);
    assert_eq!(
        serde_json::to_value(edition).expect("the shared edition track list must serialize"),
        json!({
            "id": EDITION_ID,
            "release_group_id": GROUP_ID,
            "title": "Owned bonus edition",
            "artist_credit": credit,
            "revision": null,
            "media": [
                {
                    "position": 1,
                    "title": "Owned main disc",
                    "format": "Enhanced CD",
                    "tracks": [
                        {
                            "id": PREGAP_ID, "position": 0, "number": "0", "kind": "pregap",
                            "title": "Owned hidden introduction", "duration_ms": 5000,
                            "artist_credit": null, "recording_id": PREGAP_RECORDING_ID
                        },
                        {
                            "id": FIRST_TRACK_ID, "position": 1, "number": "A1", "kind": "audio",
                            "title": "Owned first occurrence", "duration_ms": 120000,
                            "artist_credit": credit, "recording_id": REPEATED_RECORDING_ID
                        },
                        {
                            "id": SECOND_TRACK_ID, "position": 2, "number": "A2", "kind": "audio",
                            "title": "Owned unknown-duration track", "duration_ms": null,
                            "artist_credit": null, "recording_id": SECOND_RECORDING_ID
                        },
                        {
                            "id": DATA_TRACK_ID, "position": 3, "number": "3", "kind": "data",
                            "title": "Owned data session", "duration_ms": null,
                            "artist_credit": null, "recording_id": DATA_RECORDING_ID
                        }
                    ]
                },
                {
                    "position": 2,
                    "title": null,
                    "format": null,
                    "tracks": [
                        {
                            "id": REPEATED_TRACK_ID, "position": 1, "number": "B1", "kind": "audio",
                            "title": "Owned repeated occurrence", "duration_ms": 120000,
                            "artist_credit": null, "recording_id": REPEATED_RECORDING_ID
                        },
                        {
                            "id": BONUS_TRACK_ID, "position": 2, "number": "Bonus", "kind": "audio",
                            "title": "Owned bonus track", "duration_ms": 90000,
                            "artist_credit": null, "recording_id": BONUS_RECORDING_ID
                        }
                    ]
                }
            ]
        })
    );
}
