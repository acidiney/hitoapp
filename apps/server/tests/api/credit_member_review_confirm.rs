use crate::{
    account::exchange,
    credit_member_review_preview_fixture::{Context, Library, accepted, private},
    member_search_order_library::MEMBER,
};
use axum::http::{HeaderMap, Method, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCreditRole, CreditMemberReviewContext,
    test_support::{artist_catalog_discovery_service as catalog, hang_guard::hang_guard},
};
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest, CreditRole};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

struct Confirmed {
    response: (StatusCode, HeaderMap, Value),
    context: CreditMemberReviewContext,
    selected: String,
    before_associations: Value,
    after_associations: Value,
    dispatched: Vec<(Method, String, Value)>,
}

#[tokio::test]
async fn a_featured_member_confirms_the_second_searched_namesake_without_replacing_the_whole_choice()
 {
    let mut library = Library::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "member confirmation owned setup",
                Box::pin(library.prepare()),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(&library, context.as_ref().unwrap()),
        )
        .await
        .expect("The public member confirmation workflow must finish within its existing deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    library.retire().await;
    let found = match outcome {
        Ok(found) => found,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        found.response.0,
        StatusCode::OK,
        "The current featured member must confirm the explicitly selected second namesake independently of the whole-credit choice"
    );
    private(&found.response.1);
    let id = found.response.2["id"].as_str().unwrap().to_owned();
    assert!(Uuid::parse_str(&id).is_ok());
    let expected = json!(ArtistAssociation {
        id: id.clone(),
        account_id: found.context.account_id.clone(),
        library_id: found.context.library_id.clone(),
        recording_id: found.context.recording_id.clone(),
        artist_id: found.context.artist_id.clone(),
        source_id: found.context.source_id.clone(),
        observation_id: found.context.observation_id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: found.context.provider.clone(),
        provider_artist_id: found.selected.clone(),
        evidence: ArtistAssociationEvidence::UserConfirmed,
        credit_evidence: None,
        coverage_evidence: None,
        revision: 1,
    });
    assert_eq!(found.selected, catalog::SECOND_ARTIST);
    assert_eq!(found.response.2, expected);
    let before = found.before_associations.as_array().unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0]["provider_artist_id"], catalog::FIRST_ARTIST);
    assert_ne!(before[0]["artist_id"], found.context.artist_id);
    assert_ne!(before[0]["id"], id);
    let after = found.after_associations.as_array().unwrap();
    assert_eq!(after.len(), before.len() + 1);
    let added: Vec<_> = after.iter().filter(|row| row["id"] == id).collect();
    assert_eq!(added, vec![&expected]);
    let retained: Vec<_> = after
        .iter()
        .filter(|row| row["id"] != id)
        .cloned()
        .collect();
    assert_eq!(
        serde_json::to_vec(&retained).unwrap(),
        serde_json::to_vec(&found.before_associations).unwrap(),
        "Removing only the explicitly added member association must recover the complete legacy association response"
    );
    assert_eq!(
        found.dispatched,
        vec![
            (
                Method::GET,
                "/prefix/v1/catalog/discovery/capabilities".into(),
                Value::Null
            ),
            (
                Method::POST,
                "/prefix/v1/catalog/artists/search".into(),
                json!(search())
            ),
        ],
        "Confirming the explicit selection must make no additional provider request"
    );
}

fn search() -> ArtistSearchRequest {
    ArtistSearchRequest {
        query: MEMBER.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    }
}

async fn exercise(library: &Library, owned: &Context) -> Confirmed {
    let provider = library.provider.as_ref().unwrap();
    let before = owned.snapshot().await;
    let peer_before = owned.peer_snapshot().await;
    assert_eq!(before["choice"]["revision"], 1);
    assert_eq!(before["choice"]["state"]["status"], "selected");
    assert_eq!(
        before["choice"]["state"]["association_id"],
        before["associations"][0]["id"]
    );
    assert_eq!(
        before["associations"][0]["provider_artist_id"],
        catalog::FIRST_ARTIST
    );
    assert_eq!(peer_before["recordings"]["total"], 0);
    assert_eq!(peer_before["plugins"]["installations"], json!([]));
    let origin_before = library.origin_counts().await;
    let requests_before = provider.requests().len();
    let found = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{}/search", owned.member_path),
        json!({"context": owned.context, "search": search()}),
    )
    .await;
    assert_eq!(found["status"], "available");
    assert_eq!(found["context"], owned.context);
    let captured: CreditMemberReviewContext =
        serde_json::from_value(found["context"].clone()).unwrap();
    assert!(matches!(captured.role, CreditRole::Featured));
    let page: ArtistSearchPage = serde_json::from_value(found["page"].clone()).unwrap();
    page.validate_for(&search()).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].artist.name, page.items[1].artist.name);
    assert_eq!(page.items[0].artist.id, catalog::FIRST_ARTIST);
    let selected = page.items[1].artist.id.clone();
    assert_eq!(selected, catalog::SECOND_ARTIST);
    assert_ne!(selected, page.items[0].artist.id);
    let searched_count = provider.requests().len();
    assert_eq!(searched_count, requests_before + 2);
    let response = exchange(
        &owned.app,
        "POST",
        &format!("{}/confirm", owned.member_path),
        json!({"context": captured, "provider_artist_id": selected}),
        owned.account.headers(),
    )
    .await;
    assert_eq!(
        provider.requests().len(),
        searched_count,
        "Member confirmation must require no provider HTTP, as with whole-credit confirmation"
    );
    owned.assert_context_preserved().await;
    let after = owned.snapshot().await;
    assert_eq!(
        after["library"], before["library"],
        "Confirmation must preserve collection events, activity, metadata, likes, progress, playlist occurrences, audio absence and plugin policy"
    );
    assert_eq!(
        serde_json::to_vec(&after["choice"]).unwrap(),
        serde_json::to_vec(&before["choice"]).unwrap(),
        "The complete legacy whole-credit identity choice response must remain unchanged"
    );
    assert_eq!(owned.peer_snapshot().await, peer_before);
    assert_eq!(library.origin_counts().await, origin_before);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    assert_eq!(requests.len(), searched_count);
    for request in &requests[requests_before..] {
        assert_eq!(request.headers["x-hito-plugin-protocol"], "1");
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {}", catalog::TOKEN).as_str()
        );
    }
    Confirmed {
        response,
        context: captured,
        selected,
        before_associations: before["associations"].clone(),
        after_associations: after["associations"].clone(),
        dispatched: requests[requests_before..]
            .iter()
            .map(|request| {
                (
                    request.method.clone(),
                    request.path.clone(),
                    request.body.clone(),
                )
            })
            .collect(),
    }
}
