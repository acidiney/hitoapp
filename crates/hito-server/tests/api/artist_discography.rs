use hito_core::test_support::artist_catalog_discography_service as catalog;

use crate::{
    artist_review_fixture::{self as fixture, Account, Imported},
    provider_fixture::ProviderServer,
    support,
};
use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCreditRole, ArtistDiscographyAvailability,
    ArtistReviewContext, MetadataProvider, ReadArtistDiscography,
};
use hito_plugin_sdk::{ArtistDiscographyPage, ArtistDiscographyRequest};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn an_http_account_reads_only_its_confirmed_artist_discography_without_adoption() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let origin = ProviderServer::start().await;
    let mut provider = catalog::CatalogServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(&app, &account, &origin, &provider)).catch_unwind(),
    )
    .await;
    origin.shutdown().await;
    let joined = provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The owned HTTP discography workflow must finish within its deadline"),
    }
    assert!(
        joined,
        "The owned catalog provider must close and join shutdown"
    );
}

async fn exercise(
    app: &Router,
    account: &Account,
    origin: &ProviderServer,
    provider: &catalog::CatalogServer,
) {
    let imported = Imported::create(app, account, origin).await;
    let origin_before = fixture::origin_counts(origin).await;
    assert_eq!(origin_before, [1, 2, 1, 0, 0, 0, 0, 0]);
    let installation = account
        .connect(app, &provider.url, "Owned HTTP discography", None)
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
    };
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    let (status, policy) = account
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let before = imported.snapshot(app, account).await;
    let review = format!("/api/v1/recordings/{}/artist-review", imported.recording);
    let associations = format!(
        "/api/v1/recordings/{}/artist-associations",
        imported.recording
    );
    let context = accepted(
        app,
        account,
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let captured: ArtistReviewContext = serde_json::from_value(context.clone()).unwrap();
    assert!(
        captured.account_id == account.id
            && captured.recording_id == imported.recording
            && captured.artist_id == imported.artist
            && captured.observation_id == imported.observation
            && captured.source_id.is_none()
            && captured.metadata_binding.is_some()
            && captured.provider == selected
    );
    let selection = json!({"context": context, "provider_artist_id": catalog::ARTIST_ID});
    let preview = accepted(
        app,
        account,
        &format!("{review}/preview"),
        selection.clone(),
    )
    .await;
    assert!(
        preview["status"] == "available"
            && preview["profile"] == json!(catalog::expected_profile())
    );
    let confirmed = accepted(app, account, &format!("{review}/confirm"), selection).await;
    let association = confirmed["id"].as_str().unwrap().to_owned();
    assert!(Uuid::parse_str(&association).is_ok());
    assert!(
        confirmed
            == json!(ArtistAssociation {
                id: association.clone(),
                account_id: account.id.clone(),
                library_id: captured.library_id,
                recording_id: imported.recording.clone(),
                artist_id: imported.artist.clone(),
                source_id: None,
                observation_id: imported.observation.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: selected.clone(),
                provider_artist_id: catalog::ARTIST_ID.into(),
                evidence: ArtistAssociationEvidence::UserConfirmed,
                revision: 1,
            })
    );
    assert_eq!(provider.requests().len(), 3);
    provider.enable_discography();
    let path = format!("{associations}/{association}/discography");
    let (status, headers, first) =
        fixture::exchange(app, "POST", &path, page_body(None), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    let continuation = first["continuation"].as_str().unwrap().to_owned();
    assert!(
        continuation.is_ascii()
            && !continuation.is_empty()
            && continuation.len() <= 8192
            && continuation != catalog::PROVIDER_CURSOR
    );
    assert!(
        first
            == json!(ArtistDiscographyAvailability::Available {
                association_id: association.clone(),
                page: catalog::expected_page(false),
                continuation: Some(continuation.clone()),
            })
    );
    let first_page: ArtistDiscographyPage = serde_json::from_value(first["page"].clone()).unwrap();
    first_page
        .validate_for(&ArtistDiscographyRequest {
            artist_id: catalog::ARTIST_ID.into(),
            cursor: None,
            snapshot: None,
            limit: 1,
        })
        .unwrap();
    assert!(imported.snapshot(app, account).await == before);

    let before_rejections = provider.requests();
    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let mut mismatched = account.headers();
    mismatched.insert(
        "x-hito-account",
        Uuid::new_v4().to_string().parse().unwrap(),
    );
    let mut foreign_origin = account.headers();
    foreign_origin.insert("origin", "https://untrusted.example".parse().unwrap());
    let mut missing_guard = account.headers();
    missing_guard.remove("x-hito-request");
    for (headers, expected) in [
        (anonymous, StatusCode::UNAUTHORIZED),
        (mismatched, StatusCode::FORBIDDEN),
        (foreign_origin, StatusCode::FORBIDDEN),
        (missing_guard, StatusCode::FORBIDDEN),
    ] {
        let (status, _, _) = fixture::exchange(app, "POST", &path, page_body(None), headers).await;
        assert_eq!(status, expected);
    }
    let mut changed = continuation.clone().into_bytes();
    let middle = changed.len() / 2;
    changed[middle] = if changed[middle] == b'A' { b'B' } else { b'A' };
    for body in [
        json!({"limit": 0, "continuation": null}),
        json!({"limit": 51, "continuation": null}),
        json!({"limit": 1, "continuation": null, "provider_artist_id": catalog::ARTIST_ID}),
        json!({"limit": 1, "continuation": null, "cursor": catalog::PROVIDER_CURSOR}),
        page_body(Some(String::from_utf8(changed).unwrap())),
    ] {
        let (status, headers, _) =
            fixture::exchange(app, "POST", &path, body, account.headers()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        private(&headers);
    }
    for missing in [
        format!("{associations}/{}/discography", Uuid::new_v4()),
        format!(
            "/api/v1/recordings/{}/artist-associations/{association}/discography",
            Uuid::new_v4()
        ),
        format!(
            "/api/v1/recordings/{}/artist-associations/{association}/discography",
            imported.other_recording
        ),
    ] {
        let (status, _) = account
            .request(app, "POST", &missing, page_body(None))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    assert_eq!(provider.requests(), before_rejections);
    assert!(imported.snapshot(app, account).await == before);
    assert!(account.get(app, &associations).await == json!([confirmed.clone()]));

    let (status, headers, last) = fixture::exchange(
        app,
        "POST",
        &path,
        page_body(Some(continuation.clone())),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert!(
        last == json!(ArtistDiscographyAvailability::Available {
            association_id: association.clone(),
            page: catalog::expected_page(true),
            continuation: None,
        })
    );
    let last_page: ArtistDiscographyPage = serde_json::from_value(last["page"].clone()).unwrap();
    last_page
        .validate_for(&ArtistDiscographyRequest {
            artist_id: catalog::ARTIST_ID.into(),
            cursor: Some(catalog::PROVIDER_CURSOR.into()),
            snapshot: None,
            limit: 1,
        })
        .unwrap();
    assert!(
        first_page.items[0].title == last_page.items[0].title
            && first_page.items[0].id != last_page.items[0].id
    );
    assert!(imported.snapshot(app, account).await == before);
    let requests = provider.requests();
    assert_eq!(requests.len(), 7);
    let (pairs, remainder) = requests[3..].as_chunks::<2>();
    assert!(remainder.is_empty());
    for (pair, cursor) in pairs
        .iter()
        .zip([Value::Null, json!(catalog::PROVIDER_CURSOR)])
    {
        assert!(
            pair[0].method == Method::GET
                && pair[0].path == "/prefix/v1/catalog/capabilities"
                && pair[0].body.is_null()
        );
        assert!(
            pair[1].method == Method::POST
                && pair[1].path == "/prefix/v1/catalog/artists/discography"
        );
        assert_eq!(
            pair[1].body,
            json!({"artist_id": catalog::ARTIST_ID, "cursor": cursor, "snapshot": null, "limit": 1})
        );
    }

    let (status, revised) = account
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": policy["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(revised["revision"].as_i64().unwrap() > policy["revision"].as_i64().unwrap());
    let before_stale = provider.requests();
    let (status, headers, _) = fixture::exchange(
        app,
        "POST",
        &path,
        page_body(Some(continuation)),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    private(&headers);
    assert_eq!(provider.requests(), before_stale);
    let mut expected = before;
    expected["policy"] = revised;
    assert!(imported.snapshot(app, account).await == expected);
    assert!(account.get(app, &associations).await == json!([confirmed]));
    assert!(
        account
            .get(
                app,
                &format!(
                    "/api/v1/recordings/{}/artist-associations",
                    imported.other_recording
                )
            )
            .await
            == json!([])
    );
    assert_eq!(fixture::origin_counts(origin).await, origin_before);
    assert_eq!(provider.unexpected(), 0);
}

fn page_body(continuation: Option<String>) -> Value {
    let request = ReadArtistDiscography {
        limit: 1,
        continuation,
    };
    json!({"limit": request.limit, "continuation": request.continuation})
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
}

async fn accepted(app: &Router, account: &Account, path: &str, body: Value) -> Value {
    let (status, value) = account.request(app, "POST", path, body).await;
    assert_eq!(status, StatusCode::OK);
    value
}
