use crate::{
    artist_identity_fixture::Library, artist_identity_inertia_fixture::page,
    artist_review_fixture::exchange,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::MetadataProvider;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn inertia_group_reads_stay_local_after_disconnect_and_follow_explicit_resets_without_reactivation()
 {
    let library = Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &Library) {
    let legacy = library.owner.get(&library.app, "/api/v1/artists").await;
    let (local_request, local_receipt) = library.select(&library.local).await;
    let (_, imported_receipt) = library.select(&library.imported).await;
    let group = local_receipt["selection"]["group_id"].as_str().unwrap();
    assert_eq!(imported_receipt["selection"]["group_id"], group);
    let path = format!("/artist-groups/{group}");
    let mut before = library.snapshot().await;
    before.as_object_mut().unwrap().remove("settings");
    before.as_object_mut().unwrap().remove("plugins");
    let provider_before = library.provider_counts().await;
    let settings = library
        .owner
        .get(&library.app, "/api/v1/metadata/settings")
        .await;
    assert_eq!(library.owner.request(&library.app, "PUT", "/api/v1/metadata/settings", json!({"revision": settings["revision"], "enabled": false, "providers": settings["providers"]})).await.0, StatusCode::OK);
    for html in [false, true] {
        let detail = page(&library.app, &library.owner, &path, None, html).await;
        assert_eq!(detail["props"]["detail"]["group"]["recording_count"], 2);
    }
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
    let partial = page(
        &library.app,
        &library.owner,
        &path,
        Some(("ArtistIdentityGroupPage", "detail")),
        false,
    )
    .await;
    assert_eq!(partial["props"]["detail"]["group"]["recording_count"], 2);
    assert_eq!(
        partial["props"]["detail"]["group"]["provider"],
        json!(library.provider)
    );
    let local = format!("/api/v1/recordings/{}/artist-identity", library.local);
    let reset = json!({"request_id": Uuid::new_v4(), "expected_revision": 1});
    let (status, reset_receipt) = library
        .owner
        .request(
            &library.app,
            "POST",
            &format!("{local}/reset"),
            reset.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset_receipt["revision"], 2);
    let detail = page(&library.app, &library.owner, &path, None, true).await;
    assert_eq!(detail["props"]["detail"]["group"]["recording_count"], 1);
    assert_eq!(
        detail["props"]["detail"]["group"]["name"]["recording_id"],
        library.imported
    );
    assert_eq!(
        detail["props"]["detail"]["recordings"]["items"][0]["recording"]["id"],
        library.imported
    );
    let (status, replay) = library
        .owner
        .request(&library.app, "PUT", &local, local_request)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, local_receipt);
    let retained = page(
        &library.app,
        &library.owner,
        "/artists",
        Some(("ArtistIndexPage", "identity_groups")),
        false,
    )
    .await;
    assert_eq!(
        retained["props"]["identity_groups"]["items"][0]["recording_count"],
        1
    );
    let imported = format!(
        "/api/v1/recordings/{}/artist-identity/reset",
        library.imported
    );
    assert_eq!(
        library
            .owner
            .request(
                &library.app,
                "POST",
                &imported,
                json!({"request_id": Uuid::new_v4(), "expected_revision": 1})
            )
            .await
            .0,
        StatusCode::OK
    );
    let (status, replay) = library
        .owner
        .request(&library.app, "POST", &format!("{local}/reset"), reset)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, reset_receipt);
    for html in [false, true] {
        let index = page(&library.app, &library.owner, "/artists", None, html).await;
        assert_eq!(index["props"]["artists"], legacy);
        assert_eq!(
            index["props"]["identity_groups"],
            json!({"items": [], "total": 0, "offset": 0, "limit": 60})
        );
        let mut headers = library.owner.headers();
        if !html {
            headers.insert("x-inertia", "true".parse().unwrap());
        }
        let (status, _, error) = exchange(&library.app, "GET", &path, Value::Null, headers).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error, json!({"error": "Resource not found"}));
    }
    let mut headers = library.owner.headers();
    headers.insert("x-inertia", "true".parse().unwrap());
    headers.insert(
        "x-inertia-partial-component",
        "ArtistIdentityGroupPage".parse().unwrap(),
    );
    headers.insert("x-inertia-partial-data", "detail".parse().unwrap());
    let (status, _, error) = exchange(&library.app, "GET", &path, Value::Null, headers).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error, json!({"error": "Resource not found"}));
    let mut after = library.snapshot().await;
    after.as_object_mut().unwrap().remove("settings");
    after.as_object_mut().unwrap().remove("plugins");
    assert_eq!(after, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
