use crate::artist_identity_fixture::Library;
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::MetadataProvider;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn http_accepted_identity_reads_reset_and_replay_remain_local_without_a_metadata_connection()
{
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let (local_request, local_receipt) = library.select(&library.local).await;
    let (imported_request, imported_receipt) = library.select(&library.imported).await;
    let group = local_receipt["selection"]["group_id"].as_str().unwrap();
    assert_eq!(imported_receipt["selection"]["group_id"], group);
    let detail = format!("/api/v1/artist-identity-groups/{group}");
    let local = format!("/api/v1/recordings/{}/artist-identity", library.local);
    let imported = format!("/api/v1/recordings/{}/artist-identity", library.imported);
    let mut before = library.snapshot().await;
    before.as_object_mut().unwrap().remove("settings");
    before.as_object_mut().unwrap().remove("plugins");
    let provider_before = library.provider_counts().await;
    let settings = library
        .owner
        .get(&library.app, "/api/v1/metadata/settings")
        .await;
    assert_eq!(library.owner.request(&library.app, "PUT", "/api/v1/metadata/settings", json!({"revision": settings["revision"], "enabled": false, "providers": settings["providers"]})).await.0, StatusCode::OK);
    assert_eq!(
        library.owner.get(&library.app, &detail).await["recordings"]["total"],
        2
    );
    assert_eq!(
        library
            .owner
            .get(&library.app, "/api/v1/artist-identity-groups")
            .await["items"][0]["recording_count"],
        2
    );
    assert_eq!(
        library.owner.get(&library.app, &imported).await["state"]["status"],
        "selected"
    );
    let reset_imported = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let (status, reset_receipt) = library
        .owner
        .request(
            &library.app,
            "POST",
            &format!("{imported}/reset"),
            reset_imported.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset_receipt["revision"], 2);
    for (method, path, body, expected) in [
        (
            "PUT",
            imported.clone(),
            imported_request.clone(),
            imported_receipt,
        ),
        (
            "POST",
            format!("{imported}/reset"),
            reset_imported,
            reset_receipt,
        ),
    ] {
        let (status, replay) = library
            .owner
            .request(&library.app, method, &path, body)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(replay, expected);
    }
    let mut new_imported = imported_request;
    new_imported["request_id"] = json!(Uuid::new_v4());
    new_imported["expected_revision"] = json!(2);
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &imported, new_imported)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        library.owner.get(&library.app, &imported).await,
        json!({"recording_id": library.imported, "revision": 2, "state": {"status": "unselected"}})
    );
    let settings = library
        .owner
        .get(&library.app, "/api/v1/metadata/settings")
        .await;
    assert_eq!(library.owner.request(&library.app, "PUT", "/api/v1/metadata/settings", json!({"revision": settings["revision"], "enabled": true, "providers": settings["providers"]})).await.0, StatusCode::OK);
    let MetadataProvider::HttpPlugin { installation_id } = &library.provider else {
        unreachable!()
    };
    let plugins = library.owner.get(&library.app, "/api/v1/plugins").await;
    let installation = plugins["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == installation_id.as_str())
        .unwrap();
    assert_eq!(
        library
            .owner
            .request(
                &library.app,
                "DELETE",
                &format!("/api/v1/plugins/{installation_id}"),
                json!({"revision": installation["revision"]})
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let retained = library.owner.get(&library.app, &detail).await;
    assert_eq!(retained["recordings"]["total"], 1);
    assert_eq!(
        retained["recordings"]["items"][0]["recording"]["id"],
        library.local
    );
    assert_eq!(retained["group"]["provider"], json!(library.provider));
    assert_eq!(library.owner.get(&library.app, &local).await["revision"], 1);
    let reset_local = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let (status, reset_receipt) = library
        .owner
        .request(
            &library.app,
            "POST",
            &format!("{local}/reset"),
            reset_local.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset_receipt["revision"], 2);
    for (method, path, body, expected) in [
        ("PUT", local.clone(), local_request.clone(), local_receipt),
        ("POST", format!("{local}/reset"), reset_local, reset_receipt),
    ] {
        let (status, replay) = library
            .owner
            .request(&library.app, method, &path, body)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(replay, expected);
    }
    let mut new_local = local_request;
    new_local["request_id"] = json!(Uuid::new_v4());
    new_local["expected_revision"] = json!(2);
    assert_eq!(
        library
            .owner
            .request(&library.app, "PUT", &local, new_local)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        library.owner.get(&library.app, &local).await,
        json!({"recording_id": library.local, "revision": 2, "state": {"status": "unselected"}})
    );
    assert_eq!(
        library
            .owner
            .get(&library.app, "/api/v1/artist-identity-groups")
            .await["total"],
        0
    );
    assert_eq!(
        library
            .owner
            .request(&library.app, "GET", &detail, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut after = library.snapshot().await;
    after.as_object_mut().unwrap().remove("settings");
    after.as_object_mut().unwrap().remove("plugins");
    assert_eq!(after, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
