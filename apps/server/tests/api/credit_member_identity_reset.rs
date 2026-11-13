use crate::{
    account::exchange,
    credit_member_identity_reset_fixture::{Checkpoint, Reads, Response, Scenario, accepted},
    credit_member_review_preview_fixture::{Context, Library, private},
    provider_fixture::ProviderServer,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{
    ChooseCreditMemberIdentity, CreditMemberIdentityChoiceReceipt, CreditMemberReviewContext,
    ResetRecordingArtistIdentity,
    test_support::{artist_catalog_discovery_service as catalog, hang_guard::hang_guard},
};
use hito_plugin_sdk::{ArtistProfileRequest, CreditRole};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

struct Chosen {
    context: CreditMemberReviewContext,
    association: Value,
    receipt: Value,
    path: String,
}

struct Observed {
    main: Chosen,
    featured: Chosen,
    request_id: Uuid,
    before: Checkpoint,
    after: Checkpoint,
    replayed: Checkpoint,
    main_before: Reads,
    main_after: Reads,
    main_replayed: Reads,
    featured_before: Response,
    featured_after: Response,
    featured_replayed: Response,
    response: Response,
    replay: Response,
}

#[tokio::test]
async fn resetting_a_featured_identity_replays_without_resetting_main_or_whole_credit() {
    let mut library = Library::new();
    let mut context = None;
    let mut origin = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "member reset owned setup",
                Box::pin(async {
                    let owned = library.prepare().await;
                    origin = Some(ProviderServer::start().await);
                    owned
                }),
            )
            .await,
        );
        let scenario = hang_guard(
            "member reset public structured adoption",
            Box::pin(Scenario::prepare(
                context.as_ref().unwrap(),
                origin.as_ref().unwrap(),
            )),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(
                &library,
                context.as_ref().unwrap(),
                origin.as_ref().unwrap(),
                &scenario,
            ),
        )
        .await
        .expect("The public member reset workflow must finish within its existing deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    if let Some(origin) = origin.take() {
        origin.shutdown().await;
    }
    library.retire().await;
    let found = match outcome {
        Ok(found) => found,
        Err(panic) => resume_unwind(panic),
    };
    found.after.assert_private_preserved(&found.before);
    found.replayed.assert_private_preserved(&found.after);
    assert_eq!(found.after.groups, found.before.groups);
    assert_eq!(found.replayed.groups, found.after.groups);
    assert_main_preserved(&found.main_after, &found.main_before);
    assert_main_preserved(&found.main_replayed, &found.main_before);
    assert_group_witnesses(&found);
    assert_eq!(
        found.response.0,
        StatusCode::OK,
        "The chosen Featured member must reset independently through its own public reset route"
    );
    private(&found.response.1);
    let expected = CreditMemberIdentityChoiceReceipt {
        request_id: found.request_id,
        recording_id: found.featured.context.recording_id.clone(),
        artist_id: found.featured.context.artist_id.clone(),
        role: CreditRole::Featured,
        revision: 2,
        selection: None,
    };
    assert_eq!(found.response.2, json!(expected));
    assert_current(&found.featured_before, &found.featured, true, 1);
    assert_current(&found.featured_after, &found.featured, false, 2);
    assert_eq!(found.replay.0, StatusCode::OK);
    private(&found.replay.1);
    assert_eq!(
        serde_json::to_vec(&found.replay.2).unwrap(),
        serde_json::to_vec(&found.response.2).unwrap(),
        "Replaying the exact reset request must preserve its receipt"
    );
    assert_current(&found.featured_replayed, &found.featured, false, 2);
    assert_eq!(found.featured_replayed.2, found.featured_after.2);
}

async fn exercise(
    library: &Library,
    owned: &Context,
    origin: &ProviderServer,
    scenario: &Scenario,
) -> Observed {
    let main_path = format!(
        "/api/v1/recordings/{}/credit-members/{}/artist-review",
        scenario.recording, scenario.main.artist_id
    );
    let main = choose_member(
        owned,
        &main_path,
        json!(scenario.main),
        catalog::SECOND_ARTIST,
    )
    .await;
    assert!(matches!(main.context.role, CreditRole::Main));
    library.provider.as_ref().unwrap().mismatched_profile();
    let featured = choose_member(
        owned,
        &format!(
            "/api/v1/recordings/{}/credit-members/{}/artist-review",
            scenario.recording, scenario.featured.artist_id
        ),
        json!(scenario.featured),
        catalog::FIRST_ARTIST,
    )
    .await;
    assert!(matches!(featured.context.role, CreditRole::Featured));
    assert_ne!(main.context.artist_id, featured.context.artist_id);
    assert_ne!(main.association["id"], featured.association["id"]);
    assert_ne!(
        main.receipt["selection"]["group_id"],
        featured.receipt["selection"]["group_id"]
    );
    scenario.assert_contexts_current(owned).await;
    let before = Checkpoint::capture(library, owned, origin, scenario).await;
    let main_group = main.receipt["selection"]["group_id"].as_str().unwrap();
    let main_before = Reads::capture(owned, scenario, &main.path, main_group).await;
    let featured_before = current(owned, &featured.path).await;
    assert_current(&main_before.current, &main, true, 1);
    assert_current(&featured_before, &featured, true, 1);
    assert_eq!(before.state["choice"]["revision"], 1);
    assert_eq!(before.state["choice"]["state"]["status"], "selected");
    assert_ne!(
        before.state["choice"]["state"]["association_id"],
        featured.association["id"]
    );
    assert_ne!(
        before.state["choice"]["state"]["association_id"],
        main.association["id"]
    );
    assert_eq!(
        before.state["choice"]["state"]["group_id"],
        featured.receipt["selection"]["group_id"]
    );
    let request_id = Uuid::new_v4();
    let request = json!(ResetRecordingArtistIdentity {
        request_id,
        expected_revision: 1,
    });
    let reset_path = format!("{}/reset", featured.path);
    let response = exchange(
        &owned.app,
        "POST",
        &reset_path,
        request.clone(),
        owned.account.headers(),
    )
    .await;
    let featured_after = current(owned, &featured.path).await;
    let main_after = Reads::capture(owned, scenario, &main.path, main_group).await;
    let after = Checkpoint::capture(library, owned, origin, scenario).await;
    let replay = exchange(
        &owned.app,
        "POST",
        &reset_path,
        request,
        owned.account.headers(),
    )
    .await;
    let featured_replayed = current(owned, &featured.path).await;
    let main_replayed = Reads::capture(owned, scenario, &main.path, main_group).await;
    scenario.assert_contexts_current(owned).await;
    let replayed = Checkpoint::capture(library, owned, origin, scenario).await;
    Observed {
        main,
        featured,
        request_id,
        before,
        after,
        replayed,
        main_before,
        main_after,
        main_replayed,
        featured_before,
        featured_after,
        featured_replayed,
        response,
        replay,
    }
}

async fn choose_member(
    owned: &Context,
    review_path: &str,
    context: Value,
    provider_artist_id: &str,
) -> Chosen {
    let context: CreditMemberReviewContext = serde_json::from_value(context).unwrap();
    let preview = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{review_path}/preview"),
        json!({"context": context, "provider_artist_id": provider_artist_id}),
    )
    .await;
    let mut profile = catalog::expected_profile();
    profile.artist.id = provider_artist_id.into();
    profile
        .validate_for(&ArtistProfileRequest {
            artist_id: provider_artist_id.into(),
        })
        .unwrap();
    assert_eq!(
        preview,
        json!({"status": "available", "context": context, "profile": profile})
    );
    let association = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{review_path}/confirm"),
        json!({"context": preview["context"], "provider_artist_id": profile.artist.id}),
    )
    .await;
    assert_eq!(association["artist_id"], context.artist_id);
    assert_eq!(association["provider_artist_id"], provider_artist_id);
    let path = format!(
        "/api/v1/recordings/{}/credit-members/{}/artist-identity",
        context.recording_id, context.artist_id
    );
    let request_id = Uuid::new_v4();
    let request = ChooseCreditMemberIdentity {
        request_id,
        expected_revision: 0,
        association_id: Uuid::parse_str(association["id"].as_str().unwrap()).unwrap(),
        context: context.clone(),
    };
    let receipt = accepted(&owned.app, &owned.account, "PUT", &path, json!(request)).await;
    let group = receipt["selection"]["group_id"].as_str().unwrap();
    assert!(Uuid::parse_str(group).is_ok());
    assert_eq!(
        receipt,
        json!({"request_id": request_id, "recording_id": context.recording_id,
            "artist_id": context.artist_id, "role": context.role, "revision": 1,
            "selection": {"group_id": group, "association_id": association["id"]}})
    );
    Chosen {
        context,
        association,
        receipt,
        path,
    }
}

async fn current(owned: &Context, path: &str) -> Response {
    exchange(
        &owned.app,
        "GET",
        path,
        Value::Null,
        owned.account.headers(),
    )
    .await
}

fn assert_current(response: &Response, chosen: &Chosen, selected: bool, revision: i64) {
    assert_eq!(
        response.0,
        StatusCode::OK,
        "GET {} returned HTTP {}",
        chosen.path,
        response.0
    );
    private(&response.1);
    let state = if selected {
        json!({"status": "selected", "group_id": chosen.receipt["selection"]["group_id"],
            "association_id": chosen.association["id"]})
    } else {
        json!({"status": "unselected"})
    };
    assert_eq!(
        response.2,
        json!({"recording_id": chosen.context.recording_id, "artist_id": chosen.context.artist_id,
            "role": chosen.context.role, "revision": revision, "state": state})
    );
}

fn assert_main_preserved(after: &Reads, before: &Reads) {
    for (after, before) in [
        (&after.current, &before.current),
        (&after.detail, &before.detail),
        (&after.filtered, &before.filtered),
        (&after.peer_detail, &before.peer_detail),
    ] {
        assert_eq!(after.0, before.0);
        private(&after.1);
        private(&before.1);
        assert_eq!(after.2, before.2);
    }
}

fn assert_group_witnesses(found: &Observed) {
    assert_current(&found.main_before.current, &found.main, true, 1);
    assert_eq!(found.before.groups["total"], 2);
    let groups = found.before.groups["items"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    for group in [
        &found.before.whole_group["group"],
        &found.main_before.detail.2["group"],
    ] {
        assert_eq!(groups.iter().filter(|item| *item == group).count(), 1);
    }
    let recording = found.before.state["recordings"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == found.featured.context.recording_id)
        .unwrap();
    let original = &found.before.state["original"];
    let original_recording = original["recordings"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == original["choice"]["recording_id"])
        .unwrap();
    let whole = &found.before.state["choice"]["state"];
    let whole_association = found.before.state["associations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|association| association["id"] == whole["association_id"])
        .unwrap();
    let original_whole = &original["choice"]["state"];
    let original_association = original["associations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|association| association["id"] == original_whole["association_id"])
        .unwrap();
    let whole_group = &found.before.whole_group;
    assert_eq!(
        whole_association["provider_artist_id"],
        catalog::FIRST_ARTIST
    );
    assert_eq!(
        whole_group["group"]["provider_artist_id"],
        catalog::FIRST_ARTIST
    );
    assert_eq!(whole_group["group"]["id"], whole["group_id"]);
    assert_eq!(whole_group["group"]["id"], original_whole["group_id"]);
    assert_eq!(whole_association["artist_id"], found.main.context.artist_id);
    assert_eq!(
        whole_group["group"]["name"]["artist_id"],
        original_association["artist_id"]
    );
    assert_eq!(
        whole_group["group"]["name"]["association_id"],
        original_association["id"]
    );
    assert_eq!(whole_group["group"]["recording_count"], 2);
    assert_eq!(whole_group["recordings"]["total"], 2);
    let rows = whole_group["recordings"]["items"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for expected in [
        json!({"recording": recording, "artist_id": whole_association["artist_id"],
            "association_id": whole["association_id"], "choice_revision": 1}),
        json!({"recording": original_recording, "artist_id": original_association["artist_id"],
            "association_id": original_whole["association_id"], "choice_revision": 1}),
    ] {
        assert_eq!(rows.iter().filter(|row| **row == expected).count(), 1);
    }
    for response in [&found.main_before.detail, &found.main_before.filtered] {
        assert_eq!(response.0, StatusCode::OK);
        private(&response.1);
        assert_eq!(
            response.2["group"]["provider_artist_id"],
            catalog::SECOND_ARTIST
        );
        assert_eq!(
            response.2["group"]["id"],
            found.main.receipt["selection"]["group_id"]
        );
        assert_eq!(
            response.2["group"]["name"]["artist_id"],
            found.main.context.artist_id
        );
        assert_eq!(
            response.2["group"]["name"]["association_id"],
            found.main.association["id"]
        );
        assert_eq!(response.2["group"]["recording_count"], 1);
        assert_eq!(response.2["recordings"]["total"], 1);
        assert_eq!(
            response.2["recordings"]["items"],
            json!([{"recording": recording, "artist_id": found.main.context.artist_id,
                "association_id": found.main.association["id"], "choice_revision": 1}])
        );
    }
    assert_eq!(found.main_before.peer_detail.0, StatusCode::NOT_FOUND);
    private(&found.main_before.peer_detail.1);
    assert_eq!(
        found.main_before.peer_detail.2,
        json!({"error": "Resource not found"})
    );
}
