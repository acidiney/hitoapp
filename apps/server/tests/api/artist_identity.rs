use crate::{
    artist_identity_fixture::{ARTIST_ID, Library},
    artist_review_fixture as review,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{ChooseRecordingArtistIdentity, ResetRecordingArtistIdentity};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn explicit_http_choices_group_local_and_imported_credits_and_reset_without_catalog_mutation()
{
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
        Err(_) => panic!("The owned HTTP artist identity workflow must finish within its deadline"),
    }
}

async fn exercise(library: &Library) {
    let local_review = library.review(&library.local).await;
    let imported_review = library.review(&library.imported).await;
    assert_ne!(local_review.0.library_id, imported_review.0.library_id);
    assert_ne!(local_review.0.artist_id, imported_review.0.artist_id);
    assert!(local_review.0.source_id.is_some());
    assert!(local_review.0.metadata_binding.is_none());
    assert!(imported_review.0.source_id.is_none());
    assert!(imported_review.0.metadata_binding.is_some());
    let before = library.snapshot().await;
    let provider_before = library.provider_counts().await;
    let local_path = format!("/api/v1/recordings/{}/artist-identity", library.local);
    let imported_path = format!("/api/v1/recordings/{}/artist-identity", library.imported);
    for recording in [&library.local, &library.imported, &library.other] {
        let value = library
            .owner
            .get(
                &library.app,
                &format!("/api/v1/recordings/{recording}/artist-identity"),
            )
            .await;
        assert_eq!(
            value,
            json!({"recording_id": recording, "revision": 0, "state": {"status": "unselected"}})
        );
    }
    let empty = library
        .owner
        .get(&library.app, "/api/v1/artist-identity-groups")
        .await;
    assert_eq!(
        empty,
        json!({"items": [], "total": 0, "offset": 0, "limit": 60})
    );
    let mut accepted = Vec::new();
    for (recording, path, (context, association)) in [
        (&library.local, &local_path, local_review),
        (&library.imported, &imported_path, imported_review),
    ] {
        let request_id = Uuid::new_v4();
        let request = json!(ChooseRecordingArtistIdentity {
            request_id,
            expected_revision: 0,
            association_id: Uuid::parse_str(&association).unwrap(),
            context: context.clone(),
        });
        let (status, receipt) = library
            .owner
            .request(&library.app, "PUT", path, request.clone())
            .await;
        assert_eq!(status, StatusCode::OK);
        let group = receipt["selection"]["group_id"].as_str().unwrap();
        assert!(Uuid::parse_str(group).is_ok());
        assert_eq!(
            receipt,
            json!({
                "request_id": request_id, "recording_id": recording, "revision": 1,
                "selection": {"group_id": group, "association_id": association},
            })
        );
        let current = library.owner.get(&library.app, path).await;
        assert_eq!(
            current,
            json!({
                "recording_id": recording, "revision": 1,
                "state": {"status": "selected", "group_id": group, "association_id": association},
            })
        );
        accepted.push((request, receipt, context, association));
    }
    let group = accepted[0].1["selection"]["group_id"].as_str().unwrap();
    assert_eq!(accepted[1].1["selection"]["group_id"], group);
    let expected_group = json!({
        "id": group, "provider": library.provider, "provider_artist_id": ARTIST_ID,
        "name": {"value": review::CREDIT, "recording_id": library.local,
            "artist_id": accepted[0].2.artist_id, "association_id": accepted[0].3},
        "library_count": 2, "recording_count": 2, "release_count": 2,
    });
    let groups = library
        .owner
        .get(&library.app, "/api/v1/artist-identity-groups")
        .await;
    assert_eq!(
        groups,
        json!({"items": [expected_group], "total": 1, "offset": 0, "limit": 60})
    );
    let detail = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/artist-identity-groups/{group}"),
        )
        .await;
    assert_eq!(detail["group"], expected_group);
    for page in ["libraries", "recordings", "releases"] {
        assert_eq!(detail[page]["total"], 2);
        assert_eq!(detail[page]["offset"], 0);
        assert_eq!(detail[page]["limit"], 60);
        assert_eq!(detail[page]["items"].as_array().unwrap().len(), 2);
    }
    let mut members = detail["recordings"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert_eq!(item["choice_revision"], 1);
            (
                item["recording"]["id"].as_str().unwrap().to_owned(),
                item["association_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    members.sort();
    let mut expected_members = vec![
        (library.local.clone(), accepted[0].3.clone()),
        (library.imported.clone(), accepted[1].3.clone()),
    ];
    expected_members.sort();
    assert_eq!(members, expected_members);
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
    let (status, replay) = library
        .owner
        .request(&library.app, "PUT", &local_path, accepted[0].0.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, accepted[0].1);
    let reset_id = Uuid::new_v4();
    let reset = json!(ResetRecordingArtistIdentity {
        request_id: reset_id,
        expected_revision: 1
    });
    let (status, receipt) = library
        .owner
        .request(
            &library.app,
            "POST",
            &format!("{local_path}/reset"),
            reset.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        receipt,
        json!({"request_id": reset_id, "recording_id": library.local, "revision": 2, "selection": null})
    );
    let (status, replay) = library
        .owner
        .request(&library.app, "POST", &format!("{local_path}/reset"), reset)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, receipt);
    let current = library.owner.get(&library.app, &local_path).await;
    assert_eq!(
        current,
        json!({"recording_id": library.local, "revision": 2, "state": {"status": "unselected"}})
    );
    let groups = library
        .owner
        .get(&library.app, "/api/v1/artist-identity-groups")
        .await;
    assert_eq!(groups["total"], 1);
    assert_eq!(groups["items"][0]["id"], group);
    assert_eq!(groups["items"][0]["recording_count"], 1);
    assert_eq!(groups["items"][0]["library_count"], 1);
    assert_eq!(groups["items"][0]["release_count"], 1);
    assert_eq!(groups["items"][0]["name"]["recording_id"], library.imported);
    assert_eq!(
        library.owner.get(&library.app, &imported_path).await,
        json!({
            "recording_id": library.imported, "revision": 1,
            "state": {"status": "selected", "group_id": group, "association_id": accepted[1].3},
        })
    );
    let untouched = library
        .owner
        .get(
            &library.app,
            &format!("/api/v1/recordings/{}/artist-identity", library.other),
        )
        .await;
    assert_eq!(
        untouched,
        json!({"recording_id": library.other, "revision": 0, "state": {"status": "unselected"}})
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.provider_counts().await, provider_before);
}
