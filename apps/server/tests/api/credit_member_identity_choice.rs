use crate::{
    account::exchange,
    credit_member_identity_choice_fixture::{Checkpoint, Reads, Response, with_member_group},
    credit_member_review_preview_fixture::{Context, Library, accepted, private},
    member_search_order_library::MEMBER,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCreditRole, CreditMemberReviewContext,
    test_support::{artist_catalog_discovery_service as catalog, hang_guard::hang_guard},
};
use hito_plugin_sdk::CreditRole;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

struct Observed {
    context: CreditMemberReviewContext,
    association: Value,
    request_id: Uuid,
    before: Checkpoint,
    confirmed: Checkpoint,
    response: Response,
    chosen: Checkpoint,
    reads: Option<Reads>,
    replay: Option<Response>,
    replayed: Option<Checkpoint>,
    replayed_reads: Option<Reads>,
}

#[tokio::test]
async fn a_confirmed_featured_member_has_its_own_choice_group_and_exact_replay() {
    let mut library = Library::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(hang_guard("member choice owned setup", Box::pin(library.prepare())).await);
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(&library, context.as_ref().unwrap()),
        )
        .await
        .expect("The public member choice workflow must finish within its existing deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    library.retire().await;
    let found = match outcome {
        Ok(found) => found,
        Err(panic) => resume_unwind(panic),
    };
    assert_confirmation(&found);
    found.chosen.assert_private_preserved(&found.confirmed);
    if found.response.0 != StatusCode::OK {
        assert_eq!(found.chosen.groups, found.confirmed.groups);
    }
    assert_eq!(
        found.response.0,
        StatusCode::OK,
        "The confirmed featured member must choose a separate identity whose group contains its recording"
    );
    private(&found.response.1);
    let group = found.response.2["selection"]["group_id"].as_str().unwrap();
    assert!(Uuid::parse_str(group).is_ok());
    assert_ne!(
        found.response.2["selection"]["group_id"],
        found.before.state["choice"]["state"]["group_id"]
    );
    assert_eq!(
        found.response.2,
        json!({
            "request_id": found.request_id, "recording_id": found.context.recording_id,
            "artist_id": found.context.artist_id, "role": "featured", "revision": 1,
            "selection": {"group_id": group, "association_id": found.association["id"]},
        })
    );
    let expected_group = json!({
        "id": group, "provider": found.context.provider,
        "provider_artist_id": catalog::SECOND_ARTIST,
        "name": {"value": MEMBER, "recording_id": found.context.recording_id,
            "artist_id": found.context.artist_id, "association_id": found.association["id"]},
        "library_count": 1, "recording_count": 1, "release_count": 0,
    });
    assert_eq!(
        found.chosen.groups,
        with_member_group(&found.confirmed.groups, &expected_group),
        "The complete public group list must gain exactly the distinct member-only group"
    );
    assert_reads(&found, found.reads.as_ref().unwrap(), &expected_group);
    let replay = found.replay.as_ref().unwrap();
    assert_eq!(replay.0, StatusCode::OK);
    private(&replay.1);
    assert_eq!(
        serde_json::to_vec(&replay.2).unwrap(),
        serde_json::to_vec(&found.response.2).unwrap()
    );
    let replayed = found.replayed.as_ref().unwrap();
    replayed.assert_private_preserved(&found.chosen);
    assert_eq!(replayed.groups, found.chosen.groups);
    let replayed_reads = found.replayed_reads.as_ref().unwrap();
    assert_reads(&found, replayed_reads, &expected_group);
    let reads = found.reads.as_ref().unwrap();
    assert_eq!(replayed_reads.current.2, reads.current.2);
    assert_eq!(replayed_reads.detail.2, reads.detail.2);
    assert_eq!(replayed_reads.filtered.2, reads.filtered.2);
}

async fn exercise(library: &Library, owned: &Context) -> Observed {
    let before = Checkpoint::capture(library, owned).await;
    let context: CreditMemberReviewContext = serde_json::from_value(owned.context.clone()).unwrap();
    assert!(matches!(context.role, CreditRole::Featured));
    assert_eq!(context.credited_name, MEMBER);
    assert_eq!(before.state["choice"]["revision"], 1);
    assert_eq!(before.state["choice"]["state"]["status"], "selected");
    assert_eq!(before.state["associations"].as_array().unwrap().len(), 1);
    assert_eq!(
        before.state["associations"][0]["provider_artist_id"],
        catalog::FIRST_ARTIST
    );
    assert_eq!(before.groups["total"], 1);
    assert_eq!(before.groups["items"].as_array().unwrap().len(), 1);
    assert_eq!(before.groups["items"][0], before.whole_group["group"]);
    assert_eq!(
        before.whole_group["group"]["provider_artist_id"],
        catalog::FIRST_ARTIST
    );
    assert_ne!(catalog::FIRST_ARTIST, catalog::SECOND_ARTIST);
    let association = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{}/confirm", owned.member_path),
        json!({"context": context, "provider_artist_id": catalog::SECOND_ARTIST}),
    )
    .await;
    owned.assert_context_preserved().await;
    let confirmed = Checkpoint::capture(library, owned).await;
    let choice_path = format!(
        "/api/v1/recordings/{}/credit-members/{}/artist-identity",
        context.recording_id, context.artist_id
    );
    let request_id = Uuid::new_v4();
    let request = json!({
        "request_id": request_id, "expected_revision": 0,
        "association_id": association["id"], "context": context,
    });
    let response = exchange(
        &owned.app,
        "PUT",
        &choice_path,
        request.clone(),
        owned.account.headers(),
    )
    .await;
    let chosen = Checkpoint::capture(library, owned).await;
    let (reads, replay, replayed, replayed_reads) = if response.0 == StatusCode::OK {
        let group = response.2["selection"]["group_id"].as_str().unwrap();
        let reads = Reads::capture(owned, &choice_path, group).await;
        let replay = exchange(
            &owned.app,
            "PUT",
            &choice_path,
            request,
            owned.account.headers(),
        )
        .await;
        let replayed_reads = Reads::capture(owned, &choice_path, group).await;
        owned.assert_context_preserved().await;
        let replayed = Checkpoint::capture(library, owned).await;
        (
            Some(reads),
            Some(replay),
            Some(replayed),
            Some(replayed_reads),
        )
    } else {
        (None, None, None, None)
    };
    Observed {
        context,
        association,
        request_id,
        before,
        confirmed,
        response,
        chosen,
        reads,
        replay,
        replayed,
        replayed_reads,
    }
}

fn assert_confirmation(found: &Observed) {
    let association = &found.association;
    let id = association["id"].as_str().unwrap();
    assert!(Uuid::parse_str(id).is_ok());
    assert_ne!(
        association["id"],
        found.before.state["associations"][0]["id"]
    );
    assert_ne!(
        association["artist_id"],
        found.before.state["associations"][0]["artist_id"]
    );
    assert_eq!(
        *association,
        json!(ArtistAssociation {
            id: id.into(),
            account_id: found.context.account_id.clone(),
            library_id: found.context.library_id.clone(),
            recording_id: found.context.recording_id.clone(),
            artist_id: found.context.artist_id.clone(),
            source_id: found.context.source_id.clone(),
            observation_id: found.context.observation_id.clone(),
            role: ArtistCreditRole::RecordingArtist,
            provider: found.context.provider.clone(),
            provider_artist_id: catalog::SECOND_ARTIST.into(),
            evidence: ArtistAssociationEvidence::UserConfirmed,
            credit_evidence: None,
            coverage_evidence: None,
            revision: 1,
        })
    );
    let rows = found.confirmed.state["associations"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().filter(|row| *row == association).count(), 1);
    let retained: Vec<_> = rows.iter().filter(|row| row["id"] != id).cloned().collect();
    let mut prior = found.confirmed.state.clone();
    prior["associations"] = json!(retained);
    assert_eq!(
        prior["changes"]["revision"].as_i64().unwrap(),
        found.before.state["changes"]["revision"].as_i64().unwrap() + 1
    );
    assert_eq!(
        prior["changes"]["undo"],
        json!({"association_id": id, "linked": false})
    );
    prior["changes"]["revision"] = found.before.state["changes"]["revision"].clone();
    prior["changes"]["undo"] = found.before.state["changes"]["undo"].clone();
    assert_eq!(prior, found.before.state);
    found.confirmed.assert_surroundings_preserved(&found.before);
    assert_eq!(found.confirmed.groups, found.before.groups);
}

fn assert_reads(found: &Observed, reads: &Reads, expected_group: &Value) {
    for response in [&reads.current, &reads.detail, &reads.filtered] {
        assert_eq!(response.0, StatusCode::OK);
        private(&response.1);
    }
    assert_eq!(
        reads.current.2,
        json!({
            "recording_id": found.context.recording_id, "artist_id": found.context.artist_id,
            "role": "featured", "revision": 1, "state": {"status": "selected",
                "group_id": expected_group["id"], "association_id": found.association["id"]},
        })
    );
    let recording = &found.before.state["library"]["recordings"]["items"][0];
    assert_eq!(recording["id"], found.context.recording_id);
    assert_eq!(recording["library_id"], found.context.library_id);
    assert_eq!(recording["available"], false);
    for (response, limit) in [(&reads.detail, 60), (&reads.filtered, 1)] {
        assert_eq!(
            response.2,
            json!({
                "group": expected_group,
                "libraries": {"items": [{"library_id": found.context.library_id,
                    "recording_count": 1, "release_count": 0}], "total": 1, "offset": 0, "limit": limit},
                "recordings": {"items": [{"recording": recording, "artist_id": found.context.artist_id,
                    "association_id": found.association["id"], "choice_revision": 1}],
                    "total": 1, "offset": 0, "limit": limit},
                "releases": {"items": [], "total": 0, "offset": 0, "limit": limit},
            }),
            "Existing group detail and its library-filtered recording page must expose exactly the member recording"
        );
    }
    assert_eq!(reads.peer_detail.0, StatusCode::NOT_FOUND);
    private(&reads.peer_detail.1);
    assert_eq!(reads.peer_detail.2, json!({"error": "Resource not found"}));
}
