use crate::artist_identity_fixture::Library;
use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn http_credit_identity_applies_one_reviewed_artist_with_typed_counts_and_replay() {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn identity(library: &Library, recording: &str) -> Value {
    library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/recordings/{recording}/artist-identity"),
        )
        .await
}

async fn exercise(library: &Library) {
    let (_, accepted) = library.select(&library.local).await;
    let group = accepted["selection"]["group_id"].as_str().unwrap();
    let recordings = library.owner.get(&library.app, "/api/v1/recordings").await;
    let local = recordings["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|recording| recording["id"] == library.local)
        .unwrap()
        .clone();
    let credits = library.owner.get(&library.app, "/api/v1/artists").await;
    let credit = credits["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artist| artist["library_id"] != local["library_id"])
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/api/v1/artists/{credit}/artist-identity");
    let mut before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let body = json!({"request_id": Uuid::new_v4(), "group_id": group});
    let (status, receipt) = library
        .owner
        .request(&library.app, "POST", &path, body.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["request_id"], body["request_id"]);
    assert_eq!(receipt["artist_id"], credit);
    assert_eq!(receipt["group_id"], group);
    assert_eq!(
        (
            &receipt["applied"],
            &receipt["unchanged"],
            &receipt["skipped_different"],
            &receipt["skipped_unavailable"],
            &receipt["next_after"],
        ),
        (&json!(2), &json!(0), &json!(0), &json!(0), &Value::Null)
    );
    for recording in [&library.imported, &library.other] {
        let current = identity(library, recording).await;
        assert_eq!(current["state"]["status"], "selected");
        assert_eq!(current["state"]["group_id"], group);
        assert_eq!(current["revision"], 1);
    }
    let (status, replay) = library
        .owner
        .request(&library.app, "POST", &path, body.clone())
        .await;
    assert_eq!((status, replay), (StatusCode::OK, receipt));
    let mut changed = body.clone();
    changed["limit"] = json!(5);
    assert_eq!(
        library
            .owner
            .request(&library.app, "POST", &path, changed)
            .await
            .0,
        StatusCode::CONFLICT
    );
    for (bad, expected) in [
        (
            json!({"request_id": Uuid::new_v4(), "group_id": group, "limit": 0}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"request_id": Uuid::new_v4(), "group_id": group, "limit": 101}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"request_id": Uuid::new_v4(), "group_id": "missing"}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"request_id": Uuid::new_v4(), "group_id": group, "extra": 1}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        assert_eq!(
            library
                .owner
                .request(&library.app, "POST", &path, bad)
                .await
                .0,
            expected
        );
    }
    assert_eq!(
        library
            .peer
            .request(
                &library.app,
                "POST",
                &path,
                json!({"request_id": Uuid::new_v4(), "group_id": group})
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        library
            .owner
            .request(
                &library.app,
                "POST",
                "/api/v1/artists/missing/artist-identity",
                json!({"request_id": Uuid::new_v4(), "group_id": group})
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(library.provider_counts().await, provider_before);
    let mut after = library.snapshot().await;
    for snapshot in [&mut before, &mut after] {
        snapshot.as_object_mut().unwrap().remove("associations");
    }
    assert_eq!(after, before, "Only reviewed associations are added");
}
