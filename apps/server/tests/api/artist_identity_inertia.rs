use crate::{
    artist_identity_fixture::{ARTIST_ID, Library},
    artist_identity_inertia_fixture::page,
    artist_review_fixture as review,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{ArtistIdentityChoiceReceipt, ChooseRecordingArtistIdentity};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

#[tokio::test]
async fn inertia_browses_selected_identity_groups_alongside_unchanged_local_and_imported_credits() {
    let library = Library::prepare().await;
    let result = tokio::time::timeout(
        Duration::from_secs(45),
        AssertUnwindSafe(exercise(&library)).catch_unwind(),
    )
    .await;
    library.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned artist group Inertia workflow must finish within its deadline"),
    }
}

async fn exercise(library: &Library) {
    let legacy = library.owner.get(&library.app, "/api/v1/artists").await;
    let (local_request, local_receipt) = library.select(&library.local).await;
    let (imported_request, imported_receipt) = library.select(&library.imported).await;
    let local: ChooseRecordingArtistIdentity = serde_json::from_value(local_request).unwrap();
    let imported: ChooseRecordingArtistIdentity = serde_json::from_value(imported_request).unwrap();
    let local_receipt: ArtistIdentityChoiceReceipt = serde_json::from_value(local_receipt).unwrap();
    let imported_receipt: ArtistIdentityChoiceReceipt =
        serde_json::from_value(imported_receipt).unwrap();
    let selected = local_receipt.selection.as_ref().unwrap();
    let group = &selected.group_id;
    assert_eq!(
        imported_receipt.selection.as_ref().unwrap().group_id,
        *group
    );
    assert_ne!(local.context.library_id, imported.context.library_id);
    assert_ne!(local.context.artist_id, imported.context.artist_id);
    for artist in [&local.context.artist_id, &imported.context.artist_id] {
        assert!(
            legacy["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["id"] == *artist)
        );
    }
    let covered = [&local.context.artist_id];
    let remaining = library.owner.get(&library.app, "/api/v1/artists").await;
    let expected: Vec<_> = legacy["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| !covered.contains(&&item["id"].as_str().unwrap().to_owned()))
        .cloned()
        .collect();
    assert_eq!(remaining["items"], json!(expected));
    assert_eq!(
        remaining["total"].as_i64().unwrap(),
        legacy["total"].as_i64().unwrap() - 1,
        "A credit fully represented by the reviewed identity leaves the credit index, while the imported credit still holds an unreviewed recording"
    );
    for id in covered {
        let (status, _) = library
            .owner
            .request(
                &library.app,
                "GET",
                &format!("/api/v1/artists/{id}"),
                json!(null),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "Covered credit pages stay reachable"
        );
    }
    let preserved = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let groups = library
        .owner
        .get(&library.app, "/api/v1/artist-identity-groups")
        .await;
    assert_eq!(groups["total"], 1);
    assert_eq!(groups["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        groups["items"][0],
        json!({
            "id": group, "provider": library.provider, "provider_artist_id": ARTIST_ID,
            "name": {"value": review::CREDIT, "recording_id": library.local,
                "artist_id": local.context.artist_id, "association_id": selected.association_id},
            "library_count": 2, "recording_count": 2, "release_count": 2,
        })
    );
    let artists = library
        .owner
        .get(&library.app, "/api/v1/artists?q=Owned&offset=0&limit=1")
        .await;
    let group_page = library
        .owner
        .get(
            &library.app,
            "/api/v1/artist-identity-groups?q=Owned&offset=0&limit=1",
        )
        .await;
    let index = page(
        &library.app,
        &library.owner,
        "/artists?q=Owned&offset=0&group_offset=0&limit=1",
        None,
        false,
    )
    .await;
    assert_eq!(index["component"], "ArtistIndexPage");
    assert_eq!(index["props"]["artists"], artists);
    assert_eq!(index["props"]["query"], "Owned");
    let shown: Vec<_> = index["props"]["artists"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert!(
        !shown.contains(&local.context.artist_id.as_str()),
        "The index shows the reviewed identity once instead of its fully represented local credit"
    );

    assert_eq!(index["props"]["identity_groups"], group_page);

    let next = page(
        &library.app,
        &library.owner,
        "/artists?q=Owned&offset=0&group_offset=1&limit=1",
        None,
        false,
    )
    .await;
    let empty = library
        .owner
        .get(
            &library.app,
            "/api/v1/artist-identity-groups?q=Owned&offset=1&limit=1",
        )
        .await;
    assert_eq!(next["props"]["artists"], artists);
    assert_eq!(next["props"]["identity_groups"], empty);
    assert_eq!(
        empty,
        json!({"items": [], "total": 1, "offset": 1, "limit": 1})
    );
    let partial = page(
        &library.app,
        &library.owner,
        "/artists?q=Owned&offset=1&group_offset=0&limit=1",
        Some(("ArtistIndexPage", "identity_groups")),
        false,
    )
    .await;
    assert_eq!(partial["component"], "ArtistIndexPage");
    assert_eq!(
        partial["props"],
        json!({"errors": {}, "identity_groups": group_page})
    );
    let partial = page(
        &library.app,
        &library.owner,
        "/artists?q=Owned&offset=0&group_offset=1&limit=1",
        Some(("ArtistIndexPage", "artists")),
        false,
    )
    .await;
    assert_eq!(partial["props"], json!({"errors": {}, "artists": artists}));

    let query = "library_offset=1&recording_offset=0&release_offset=1&limit=1";
    let detail = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/artist-identity-groups/{group}?{query}"),
        )
        .await;
    assert_eq!(detail["group"], groups["items"][0]);
    for (field, offset) in [("libraries", 1), ("recordings", 0), ("releases", 1)] {
        assert_eq!(detail[field]["total"], 2);
        assert_eq!(detail[field]["offset"], offset);
        assert_eq!(detail[field]["limit"], 1);
        assert_eq!(detail[field]["items"].as_array().unwrap().len(), 1);
    }
    let path = format!("/artist-groups/{group}?{query}");
    let full = page(&library.app, &library.owner, &path, None, false).await;
    assert_eq!(full["component"], "ArtistIdentityGroupPage");
    assert_eq!(full["props"]["detail"], detail);
    assert_eq!(full["props"]["user"]["id"], library.owner.id);
    let partial = page(
        &library.app,
        &library.owner,
        &path,
        Some(("ArtistIdentityGroupPage", "detail")),
        false,
    )
    .await;
    assert_eq!(partial["component"], "ArtistIdentityGroupPage");
    assert_eq!(partial["props"], json!({"errors": {}, "detail": detail}));
    let html = page(&library.app, &library.owner, &path, None, true).await;
    assert_eq!(html, full);

    let query = format!(
        "library={}&q=Owned&offset=0&limit=1",
        local.context.library_id
    );
    let local_artists = library
        .owner
        .get(&library.app, &format!("/api/v1/artists?{query}"))
        .await;
    let local_groups = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/artist-identity-groups?{query}"),
        )
        .await;
    assert_eq!(local_groups["items"][0]["library_count"], 1);
    assert_eq!(local_groups["items"][0]["recording_count"], 1);
    assert_eq!(local_groups["items"][0]["release_count"], 1);
    let scoped = page(
        &library.app,
        &library.owner,
        &format!("/artists?{query}&group_offset=0"),
        None,
        false,
    )
    .await;
    assert_eq!(scoped["props"]["artists"], local_artists);
    assert_eq!(scoped["props"]["identity_groups"], local_groups);
    assert_eq!(library.snapshot().await, preserved);
    assert_eq!(library.provider_counts().await, provider_before);
}
