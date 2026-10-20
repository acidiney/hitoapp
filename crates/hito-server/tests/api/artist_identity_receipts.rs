use crate::artist_identity_fixture::Library;
use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::json;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn http_identity_receipts_keep_replays_immutable_across_revisions_commands_and_recordings() {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (choose, accepted) = library.select(&library.local).await;
    let (context, association) = library.review(&library.imported).await;
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let path = format!("/api/v1/recordings/{}/artist-identity", library.local);
    let reset_path = format!("{path}/reset");
    let mut changed = choose.clone();
    changed["expected_revision"] = json!(1);
    let mut stale = choose.clone();
    stale["request_id"] = json!(Uuid::new_v4());
    let reset_id = Uuid::new_v4();
    let reset = json!({"request_id": reset_id, "expected_revision": 1});
    for (method, route, body) in [
        ("PUT", path.clone(), changed),
        ("PUT", path.clone(), stale),
        (
            "POST",
            reset_path.clone(),
            json!({"request_id": choose["request_id"], "expected_revision": 1}),
        ),
        (
            "POST",
            reset_path.clone(),
            json!({"request_id": Uuid::new_v4(), "expected_revision": 0}),
        ),
        (
            "PUT",
            format!("/api/v1/recordings/{}/artist-identity", library.imported),
            json!({
                "request_id": choose["request_id"], "expected_revision": 0,
                "context": context, "association_id": association,
            }),
        ),
    ] {
        assert_eq!(
            library
                .owner
                .request(&library.app, method, &route, body)
                .await
                .0,
            StatusCode::CONFLICT
        );
    }
    let group = accepted["selection"]["group_id"].as_str().unwrap();
    let selected = |revision| {
        json!({"recording_id": library.local, "revision": revision,
        "state": {"status": "selected", "group_id": group, "association_id": accepted["selection"]["association_id"]}})
    };
    assert_eq!(library.owner.get(&library.app, &path).await, selected(1));
    let (status, reset_receipt) = library
        .owner
        .request(&library.app, "POST", &reset_path, reset.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reset_receipt,
        json!({"request_id": reset_id, "recording_id": library.local, "revision": 2, "selection": null})
    );
    let (status, replay) = library
        .owner
        .request(&library.app, "PUT", &path, choose.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, accepted);
    assert_eq!(
        library.owner.get(&library.app, &path).await,
        json!({"recording_id": library.local, "revision": 2, "state": {"status": "unselected"}})
    );
    let mut cross_command = choose.clone();
    cross_command["request_id"] = json!(reset_id);
    cross_command["expected_revision"] = json!(2);
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &path, cross_command)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let mut rechoose = choose.clone();
    rechoose["request_id"] = json!(Uuid::new_v4());
    rechoose["expected_revision"] = json!(2);
    let (status, current_receipt) = library
        .owner
        .request(&library.app, "PUT", &path, rechoose)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current_receipt["revision"], 3);
    assert_eq!(current_receipt["selection"], accepted["selection"]);
    for (method, route, body, expected) in [
        ("PUT", &path, choose, accepted.clone()),
        ("POST", &reset_path, reset.clone(), reset_receipt.clone()),
    ] {
        let (status, replay) = library
            .owner
            .request(&library.app, method, route, body)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(replay, expected);
        assert_eq!(library.owner.get(&library.app, &path).await, selected(3));
    }
    let changed_reset = json!({"request_id": reset_id, "expected_revision": 3});
    assert_eq!(
        library
            .owner
            .request(&library.app, "POST", &reset_path, changed_reset)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(library.owner.get(&library.app, &path).await, selected(3));
    assert_eq!(
        library
            .owner
            .get(
                &library.app,
                &format!("/api/v1/recordings/{}/artist-identity", library.imported)
            )
            .await,
        json!({"recording_id": library.imported, "revision": 0, "state": {"status": "unselected"}})
    );
    assert_eq!(
        library
            .owner
            .get(&library.app, "/api/v1/artist-identity-groups")
            .await["items"][0]["recording_count"],
        1
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
