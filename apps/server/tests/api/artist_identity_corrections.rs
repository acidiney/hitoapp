use crate::{artist_identity_fixture::Library, artist_review_fixture as fixture};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::artist_identity_selection_catalog as catalog;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn http_splits_undo_and_shared_corrections_use_typed_authenticated_routes() {
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

async fn post(library: &Library, path: &str, body: Value) -> (StatusCode, Value) {
    library
        .owner
        .request(&library.app, "POST", path, body)
        .await
}

async fn exercise(library: &Library) {
    let (_, accepted) = library.select(&library.local).await;
    let group = accepted["selection"]["group_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let recordings = library.owner.get(&library.app, "/api/v1/recordings").await;
    let local_library = recordings["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|recording| recording["id"] == library.local)
        .unwrap()["library_id"]
        .clone();
    let credits = library.owner.get(&library.app, "/api/v1/artists").await;
    let credit = credits["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artist| artist["library_id"] != local_library)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, _) = post(
        library,
        &format!("/api/v1/artists/{credit}/artist-identity"),
        json!({"request_id": Uuid::new_v4(), "group_id": group}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let base = format!("/api/v1/recordings/{}/artist-review", library.other);
    let (status, context) = post(
        library,
        &format!("{base}/context"),
        json!({"provider": library.provider}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post(
        library,
        &format!("{base}/confirm"),
        json!({"context": context, "provider_artist_id": catalog::BETA}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let changes = format!("/api/v1/artist-identity-groups/{group}/changes");
    let split = format!("/api/v1/artist-identity-groups/{group}/split");
    let undo = format!("/api/v1/artist-identity-groups/{group}/undo");
    let state = library.owner.get(&library.app, &changes).await;
    assert_eq!(state["revision"], 0);
    assert!(state["latest"].is_null());

    let body = json!({
        "request_id": Uuid::new_v4(), "expected_revision": 0, "recordings": [library.other],
        "destination": {"kind": "identity", "provider": library.provider, "provider_artist_id": catalog::BETA},
    });
    let mut anonymous = library.owner.headers();
    anonymous.remove("cookie");
    let mut foreign = library.owner.headers();
    foreign.insert("x-hito-account", library.peer.id.parse().unwrap());
    for (headers, expected) in [
        (anonymous, StatusCode::UNAUTHORIZED),
        (foreign, StatusCode::FORBIDDEN),
    ] {
        for (method, path) in [("GET", &changes), ("POST", &split), ("POST", &undo)] {
            let (status, _, _) =
                fixture::exchange(&library.app, method, path, body.clone(), headers.clone()).await;
            assert_eq!(status, expected);
        }
    }
    let (status, _) = library
        .peer
        .request(&library.app, "POST", &split, body.clone())
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = library
        .peer
        .request(&library.app, "GET", &changes, Value::Null)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let mut unknown = body.clone();
    unknown["unexpected_private_field"] = json!(true);
    assert_eq!(
        post(library, &split, unknown).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        post(library, &split, json!({
            "request_id": Uuid::new_v4(), "expected_revision": 0, "destination": {"kind": "credit"},
        }))
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        post(
            library,
            &split,
            json!({
                "request_id": Uuid::new_v4(), "expected_revision": 0, "recordings": [library.other],
                "destination": {"kind": "merge"},
            })
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let (status, receipt) = post(library, &split, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (
            &receipt["revision"],
            &receipt["kind"],
            &receipt["recordings"]
        ),
        (&json!(1), &json!("split"), &json!(1))
    );
    assert_eq!(
        post(library, &split, body.clone()).await,
        (StatusCode::OK, receipt.clone())
    );
    let mut changed = body.clone();
    changed["expected_revision"] = json!(1);
    assert_eq!(post(library, &split, changed).await.0, StatusCode::CONFLICT);
    let destination = receipt["destination_group_id"].as_str().unwrap();
    let moved = identity(library, &library.other).await;
    assert_eq!(moved["state"]["status"], "selected");
    assert_eq!(moved["state"]["group_id"], destination);
    let state = library.owner.get(&library.app, &changes).await;
    assert_eq!(state["revision"], 1);
    assert_eq!(state["latest"]["kind"], "split");
    assert_eq!(state["latest"]["recordings"], 1);
    assert_eq!(state["latest"]["undoable"], true);
    assert_eq!(
        state["latest"]["shareable"], false,
        "Only built-in artists can be shared"
    );

    let back = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let (status, undone) = post(library, &undo, back.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&undone["revision"], &undone["kind"]),
        (&json!(2), &json!("undo"))
    );
    assert_eq!(post(library, &undo, back).await, (StatusCode::OK, undone));
    assert_eq!(
        post(
            library,
            &undo,
            json!({"request_id": Uuid::new_v4(), "expected_revision": 1})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let restored = identity(library, &library.other).await;
    assert_eq!(restored["state"]["group_id"], group);
    assert_eq!(
        library.provider_counts().await,
        provider_before,
        "Splits make no provider request"
    );
    assert_eq!(library.snapshot().await, before);

    let promote = "/api/v1/artist-corrections/promote";
    let source = json!({"kind": "split", "group_id": group, "revision": 2});
    let request = json!({"request_id": Uuid::new_v4(), "expected_revision": 0, "source": source});
    assert_eq!(
        post(library, promote, request.clone()).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (status, _) = library
        .peer
        .request(&library.app, "POST", promote, request.clone())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = library
        .peer
        .request(
            &library.app,
            "GET",
            "/api/v1/artist-corrections",
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let list = library
        .owner
        .get(&library.app, "/api/v1/artist-corrections")
        .await;
    assert_eq!(
        list,
        json!({"revision": 0, "items": [], "total": 0, "offset": 0, "limit": 25})
    );
    let paged = library
        .owner
        .get(&library.app, "/api/v1/artist-corrections?offset=3&limit=1")
        .await;
    assert_eq!((&paged["offset"], &paged["limit"]), (&json!(3), &json!(1)));
    let (status, _) = library
        .owner
        .request(
            &library.app,
            "GET",
            "/api/v1/artist-corrections?limit=x",
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let undo_shared = "/api/v1/artist-corrections/missing/undo";
    let request = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    assert_eq!(
        post(library, undo_shared, request.clone()).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = library
        .peer
        .request(&library.app, "POST", undo_shared, request)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut anonymous = library.owner.headers();
    anonymous.remove("cookie");
    let (status, _, _) = fixture::exchange(
        &library.app,
        "GET",
        "/api/v1/artist-corrections",
        Value::Null,
        anonymous,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
