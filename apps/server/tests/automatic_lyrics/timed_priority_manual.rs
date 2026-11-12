use super::{
    setup::Prepared,
    timed_priority::{candidate, content},
};
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn a_reviewed_plain_selection_stays_exact_beside_an_automatic_timed_result() {
    let prepared = Prepared::new().await;
    let plain = candidate(
        &prepared.provider,
        "owned:manual-plain",
        Some(content(false)),
    );
    let timed = candidate(
        &prepared.provider,
        "owned:automatic-timed",
        Some(content(true)),
    );
    prepared
        .provider
        .reply("search", json!({"items":[plain,timed],"next_cursor":null}));
    let base = prepared.path().replace("/automatic", "");
    let choice = json!({"installation_id":prepared.installation["id"],"installation_revision":prepared.installation["revision"],"provider_recording_id":"owned:manual-plain"});
    let (status, review) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{base}/identities/review"),
            choice.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, identity) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":choice,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let read = json!({"selector":identity["selector"],"installation_revision":prepared.installation["revision"]});
    let before = prepared.library.snapshot().await;
    let (status, selected_before) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{base}/identities/read"),
            read.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let automatic = prepared.read().await;
    let (status, selected_after) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{base}/identities/read"),
            read,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(prepared.library.snapshot().await, before);
    assert_eq!(
        prepared
            .library
            .account
            .get(&prepared.library.app, &format!("{base}/identities"))
            .await["items"],
        json!([identity])
    );
    let calls = prepared.provider.calls();
    prepared.finish().await;
    assert_eq!(
        automatic["provider_recording"]["id"],
        "owned:automatic-timed"
    );
    assert_eq!(
        selected_before["provider_recording"]["id"],
        "owned:manual-plain"
    );
    assert_eq!(
        selected_before["document"]["lyrics"]["content"]["kind"],
        "plain"
    );
    assert_eq!(selected_before, selected_after);
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == "v2_descriptor")
            .count(),
        3,
        "Independent manual workflows must negotiate afresh outside the automatic scan"
    );
}
