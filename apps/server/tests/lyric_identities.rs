#[allow(dead_code)]
#[path = "fixtures/artist_review_account.rs"]
mod account;
#[allow(dead_code)]
#[path = "fixtures/lyric_identity_library.rs"]
mod fixture;
#[path = "fixtures/lyric_identity_provider.rs"]
mod lyric_provider;
#[allow(dead_code)]
#[path = "api/support.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn an_owner_reviews_and_reads_an_independent_lyric_identity_without_changing_sources_or_metadata()
 {
    let lyrics = lyric_provider::Provider::start().await;
    let library = fixture::Library::prepare().await;
    let installation = library
        .account
        .connect(&library.app, &lyrics.url, "Independent lyrics", None)
        .await;
    let before = library.snapshot().await;
    let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
    let (status, services) = library
        .account
        .request(
            &library.app,
            "GET",
            &format!("{base}/services"),
            Value::Null,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "The owner must list approved independent lyric services locally"
    );
    assert!(
        services["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["installation_id"] == installation["id"])
    );
    lyrics.assert_calls(&["manifest"]);
    let candidate = json!({"installation_id": installation["id"], "installation_revision": installation["revision"], "provider_recording_id": "independent:recording"});
    let (status, found) = library.account.request(&library.app, "POST", &format!("{base}/candidates"), json!({"installation_id": installation["id"], "installation_revision": installation["revision"], "query": "Explicit private search"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["items"][0]["id"], "independent:recording");
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        review["candidate"]["recording"]["id"],
        "independent:recording"
    );
    let (status, identity) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate": candidate, "review_stamp": review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(identity["selector"]["kind"], "reviewed");
    let (status, read) = library.account.request(&library.app, "POST", &format!("{base}/identities/read"), json!({"selector": identity["selector"], "installation_revision": installation["revision"]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["status"], "ready_v2");
    assert_eq!(read["selector"], identity["selector"]);
    assert_eq!(
        read["provider_recording"],
        json!({"id": "independent:recording", "revision": "independent:1"})
    );
    assert_eq!(
        read["document"]["lyrics"]["content"],
        json!({"kind": "plain", "lines": ["Owned untimed lyric text"]})
    );
    assert_eq!(
        library.snapshot().await,
        before,
        "Retaining lyrics must not mutate catalog, sources, metadata, collection, or plugin approval"
    );
    library.assert_original();
    lyrics.assert_calls(&[
        "manifest",
        "search",
        "metadata",
        "metadata",
        "v2_descriptor",
        "v2_lyrics",
    ]);
    lyrics.finish().await;
}

#[tokio::test]
async fn duplicate_accepts_share_one_identity_and_deleted_selectors_never_select_a_readded_identity()
 {
    let lyrics = lyric_provider::Provider::start().await;
    let library = fixture::Library::prepare().await;
    let installation = library
        .account
        .connect(&library.app, &lyrics.url, "Independent lyrics", None)
        .await;
    let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
    let candidate = json!({"installation_id": installation["id"], "installation_revision": installation["revision"], "provider_recording_id":"independent:recording"});
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let accept = json!({"candidate":candidate,"review_stamp":review["review_stamp"]});
    let (status, first) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            accept.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, duplicate) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            accept.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first, duplicate);
    let listed = library
        .account
        .get(&library.app, &format!("{base}/identities"))
        .await;
    assert_eq!(
        listed["items"],
        json!([first.clone()]),
        "A retained identity must have the same public data in accept and list responses"
    );
    let identity_id = first["selector"]["identity_id"].as_str().unwrap();
    let revision = first["selector"]["identity_revision"].as_i64().unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "DELETE",
            &format!("{base}/identities/{identity_id}?revision={revision}"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, replacement) = library
        .account
        .request(&library.app, "POST", &format!("{base}/identities"), accept)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(replacement["selector"], first["selector"]);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/read"),
            json!({"selector":first["selector"],"installation_revision":installation["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    lyrics.assert_calls(&["manifest", "metadata", "metadata", "metadata", "metadata"]);
    lyrics.finish().await;
}

#[tokio::test]
async fn the_eight_identity_cap_keeps_unavailable_entries_removable_and_private() {
    let lyrics = lyric_provider::Provider::start().await;
    let replacement_provider = lyric_provider::Provider::start().await;
    let library = fixture::Library::prepare().await;
    let installation = library
        .account
        .connect(&library.app, &lyrics.url, "Independent lyrics", None)
        .await;
    let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
    let mut first_accept = Value::Null;
    let mut first = Value::Null;
    for index in 0..9 {
        let candidate = json!({"installation_id":installation["id"],"installation_revision":installation["revision"],"provider_recording_id":format!("independent:recording:{index}")});
        let (status, review) = library
            .account
            .request(
                &library.app,
                "POST",
                &format!("{base}/identities/review"),
                candidate.clone(),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let accept = json!({"candidate":candidate,"review_stamp":review["review_stamp"]});
        let (status, identity) = library
            .account
            .request(
                &library.app,
                "POST",
                &format!("{base}/identities"),
                accept.clone(),
            )
            .await;
        assert_eq!(
            status,
            if index == 8 {
                StatusCode::CONFLICT
            } else {
                StatusCode::OK
            }
        );
        if index == 0 {
            first_accept = accept;
            first = identity;
        }
        if index == 7 {
            let (status, duplicate) = library
                .account
                .request(
                    &library.app,
                    "POST",
                    &format!("{base}/identities"),
                    first_accept.clone(),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                duplicate, first,
                "Exact duplicate acceptance remains idempotent at the cap"
            );
        }
    }
    let (status, _) = library
        .account
        .request(
            &library.app,
            "DELETE",
            &format!("/api/v1/plugins/{}", installation["id"].as_str().unwrap()),
            json!({"revision":installation["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let unavailable = library
        .account
        .get(&library.app, &format!("{base}/identities"))
        .await;
    let items = unavailable["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        8,
        "Unavailable retained identities must remain visible for local removal"
    );
    assert!(items.iter().all(|item| item["selectable"] == false));
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/read"),
            json!({"selector":first["selector"],"installation_revision":installation["revision"]}),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "DELETE",
            &format!(
                "{base}/identities/{}?revision=1",
                first["selector"]["identity_id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let next = library
        .account
        .connect(
            &library.app,
            &replacement_provider.url,
            "Independent lyrics",
            None,
        )
        .await;
    let candidate = json!({"installation_id":next["id"],"installation_revision":next["revision"],"provider_recording_id":"independent:recording"});
    let (status, review) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = library
        .account
        .request(
            &library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Removing an unavailable identity must free one slot"
    );
    let list = library
        .account
        .get(&library.app, &format!("{base}/identities"))
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 8);
    assert_eq!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["selectable"] == true)
            .count(),
        1
    );
    let (status, user) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/users",
            json!({"username":"peer","password":"owned private lyric peer password"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let libraries = library.account.get(&library.app, "/api/v1/libraries").await;
    let shared = libraries
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["personal"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, _) = library
        .account
        .request(
            &library.app,
            "PUT",
            &format!(
                "/api/v1/libraries/{shared}/members/{}",
                user["id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let peer =
        account::Account::login(&library.app, "peer", "owned private lyric peer password").await;
    assert_eq!(
        peer.get(&library.app, &format!("{base}/identities")).await["items"],
        json!([])
    );
    assert_eq!(
        peer.get(&library.app, &format!("{base}/services")).await["items"],
        json!([])
    );
    let mut expected = vec!["manifest"];
    expected.extend(std::iter::repeat_n("metadata", 19));
    lyrics.assert_calls(&expected);
    replacement_provider.assert_calls(&["manifest", "metadata", "metadata"]);
    library.assert_original();
    replacement_provider.finish().await;
    lyrics.finish().await;
}

#[path = "lyric_identities/authority.rs"]
mod authority;
#[path = "lyric_identities/cancellation.rs"]
mod cancellation;
#[path = "lyric_identities/erasure.rs"]
mod erasure;
#[path = "lyric_identities/isolation.rs"]
mod isolation;
#[path = "lyric_identities/logging.rs"]
mod logging;
#[allow(dead_code)]
#[path = "fixtures/timed_lyrics_logs.rs"]
mod logs;
#[path = "lyric_identities/privacy.rs"]
mod privacy;
#[path = "lyric_identities/protocol.rs"]
mod protocol;
#[path = "lyric_identities/setup.rs"]
mod setup;
#[allow(dead_code)]
#[path = "fixtures/manual_source.rs"]
mod source_provider;

#[path = "lyric_identities/lrclib.rs"]
mod lrclib;
#[path = "lyric_identities/purge.rs"]
mod purge;
#[path = "lyric_identities/schema.rs"]
mod schema;
