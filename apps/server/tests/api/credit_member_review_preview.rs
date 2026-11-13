use crate::{
    account::exchange,
    credit_member_review_preview_fixture::{Context, Library, accepted, private},
    member_search_order_library::MEMBER,
};
use axum::http::{HeaderMap, Method, StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::{
    artist_catalog_discovery_service as catalog, hang_guard::hang_guard,
};
use hito_plugin_sdk::{ArtistProfileRequest, ArtistSearchPage, ArtistSearchRequest};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

struct Previewed {
    response: (StatusCode, HeaderMap, Value),
    context: Value,
    selected: String,
    dispatched: Vec<(Method, String, Value)>,
}

#[tokio::test]
async fn a_featured_member_previews_the_second_searched_namesake_without_changing_the_whole_choice()
{
    let mut library = Library::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "member candidate preview owned setup",
                Box::pin(library.prepare()),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(&library, context.as_ref().unwrap()),
        )
        .await
        .expect("The public member preview workflow must finish within its deadline")
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
        "The current member's explicitly selected namesake must have an independent candidate preview"
    );
    private(&found.response.1);
    let profile = catalog::expected_profile();
    profile
        .validate_for(&ArtistProfileRequest {
            artist_id: found.selected.clone(),
        })
        .unwrap();
    assert_eq!(
        found.response.2,
        json!({"status": "available", "context": found.context, "profile": profile})
    );
    assert_eq!(found.selected, catalog::SECOND_ARTIST);
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
            (
                Method::GET,
                "/prefix/v1/catalog/capabilities".into(),
                Value::Null
            ),
            (
                Method::POST,
                "/prefix/v1/catalog/artists/profile".into(),
                json!({"artist_id": catalog::SECOND_ARTIST})
            ),
        ]
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

async fn exercise(library: &Library, owned: &Context) -> Previewed {
    let provider = library.provider.as_ref().unwrap();
    let before = owned.snapshot().await;
    let peer_before = owned.peer_snapshot().await;
    assert_eq!(before["choice"]["revision"], 1);
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
    let page: ArtistSearchPage = serde_json::from_value(found["page"].clone()).unwrap();
    page.validate_for(&search()).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].artist.name, page.items[1].artist.name);
    assert_eq!(page.items[0].artist.id, catalog::FIRST_ARTIST);
    let selected = page.items[1].artist.id.clone();
    assert_eq!(selected, catalog::SECOND_ARTIST);
    assert_ne!(selected, page.items[0].artist.id);
    let searched = provider.requests();
    assert_eq!(searched.len(), requests_before + 2);
    assert_eq!(searched[requests_before].method, Method::GET);
    assert_eq!(
        searched[requests_before].path,
        "/prefix/v1/catalog/discovery/capabilities"
    );
    assert_eq!(searched[requests_before].body, Value::Null);
    assert_eq!(searched[requests_before + 1].method, Method::POST);
    assert_eq!(
        searched[requests_before + 1].path,
        "/prefix/v1/catalog/artists/search"
    );
    assert_eq!(searched[requests_before + 1].body, json!(search()));
    let path = format!("{}/preview", owned.member_path);
    let body = json!({"context": found["context"], "provider_artist_id": selected});
    let denied = exchange(
        &owned.app,
        "POST",
        &path,
        body.clone(),
        owned.peer.headers(),
    )
    .await;
    assert_eq!(denied.0, StatusCode::NOT_FOUND);
    private(&denied.1);
    assert_eq!(denied.2, json!({"error": "Resource not found"}));
    assert_eq!(provider.requests().len(), searched.len());
    let response = exchange(&owned.app, "POST", &path, body, owned.account.headers()).await;
    owned.assert_context_preserved().await;
    assert_eq!(owned.snapshot().await, before);
    assert_eq!(owned.peer_snapshot().await, peer_before);
    assert_eq!(library.origin_counts().await, origin_before);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    for request in &requests[requests_before..] {
        assert_eq!(request.headers["x-hito-plugin-protocol"], "1");
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {}", catalog::TOKEN).as_str()
        );
    }
    let dispatched = requests[requests_before..]
        .iter()
        .map(|request| {
            (
                request.method.clone(),
                request.path.clone(),
                request.body.clone(),
            )
        })
        .collect();
    Previewed {
        response,
        context: owned.context.clone(),
        selected,
        dispatched,
    }
}
