use crate::{
    artist_review_fixture::{self as fixture, Account, Imported, catalog},
    provider_fixture::ProviderServer,
    support,
};
use axum::{
    Router,
    http::{Method, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability,
    ArtistCandidateSearchAvailability, ArtistCreditRole, ArtistProfileAvailability,
    ArtistReviewContext, MetadataProvider,
};
use hito_plugin_sdk::{
    ArtistProfileRequest, ArtistSearchPage, ArtistSearchRequest, CatalogArtistProfile,
};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn an_http_account_discovers_previews_and_confirms_only_its_selected_imported_artist() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let origin = ProviderServer::start().await;
    let mut provider = catalog::DiscoveryServer::start().await;
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
        Err(_) => panic!("The owned HTTP artist review workflow must finish within its deadline"),
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
    provider: &catalog::DiscoveryServer,
) {
    let imported = Imported::create(app, account, origin).await;
    let origin_before = fixture::origin_counts(origin).await;
    assert_eq!(origin_before, [1, 2, 1, 0, 0, 0, 0, 0]);
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    assert!(
        settings
            == json!({
                "revision": 0, "enabled": true, "providers": [MetadataProvider::BuiltinMusicBrainz],
                "automatic_artist_lookup": true, "automatic_artwork": true,
                "automatic_service_search": true,
            })
    );
    assert!(provider.requests().is_empty());
    let installation = account
        .connect(
            app,
            &provider.url,
            "Owned HTTP artist catalog",
            Some(catalog::TOKEN),
        )
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
    };
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
    let review_path = format!("/api/v1/recordings/{}/artist-review", imported.recording);
    let associations_path = format!(
        "/api/v1/recordings/{}/artist-associations",
        imported.recording
    );
    let other_associations = format!(
        "/api/v1/recordings/{}/artist-associations",
        imported.other_recording
    );
    let (status, value) = account
        .request(
            app,
            "POST",
            &format!("{review_path}/context"),
            json!({"provider": selected}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let context: ArtistReviewContext = serde_json::from_value(value).unwrap();
    assert!(
        context.account_id == account.id
            && context.recording_id == imported.recording
            && context.artist_id == imported.artist
            && context.credited_name == fixture::CREDIT
            && context.observation_id == imported.observation
            && context.source_id.is_none()
            && context.artist_operation_id.is_some()
            && context.provider == selected
            && json!(context.policy_revision) == policy["revision"]
    );
    let binding = context.metadata_binding.as_ref().unwrap();
    assert!(Uuid::parse_str(&binding.id).is_ok() && binding.revision == 1);
    let connection = context.connection.as_ref().unwrap();
    assert!(
        json!(connection.installation_revision) == installation["revision"]
            && json!(connection.service_id) == installation["service_id"]
            && connection.service_revision == 1
    );
    assert_eq!(provider.requests().len(), 1);
    assert!(account.get(app, &associations_path).await == json!([]));
    assert!(account.get(app, &other_associations).await == json!([]));
    let search = ArtistSearchRequest {
        query: fixture::CREDIT.into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };
    let (status, found) = account
        .request(
            app,
            "POST",
            &format!("{review_path}/search"),
            json!({"context": context, "search": search}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let page: ArtistSearchPage = serde_json::from_value(found["page"].clone()).unwrap();
    page.validate_for(&search).unwrap();
    assert!(
        page.items.len() == 2
            && page.items[0].artist.name == page.items[1].artist.name
            && page.items[0].artist.id == catalog::FIRST_ARTIST
            && page.items[1].artist.id == catalog::SECOND_ARTIST
            && page.snapshot.as_deref() == Some(catalog::SNAPSHOT)
    );
    let chosen = page.items[1].artist.id.clone();
    assert!(
        found
            == json!(ArtistCandidateSearchAvailability::Available {
                context: Box::new(context.clone()),
                page,
            })
    );
    assert!(imported.snapshot(app, account).await == before);
    assert!(account.get(app, &associations_path).await == json!([]));
    let selection = json!({"context": context, "provider_artist_id": chosen});
    let (status, preview) = account
        .request(
            app,
            "POST",
            &format!("{review_path}/preview"),
            selection.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let profile: CatalogArtistProfile = serde_json::from_value(preview["profile"].clone()).unwrap();
    profile
        .validate_for(&ArtistProfileRequest {
            artist_id: chosen.clone(),
        })
        .unwrap();
    assert!(profile == catalog::expected_profile());
    assert!(
        preview
            == json!(ArtistCandidatePreviewAvailability::Available {
                context: Box::new(context.clone()),
                profile,
            })
    );
    assert!(imported.snapshot(app, account).await == before);
    assert!(account.get(app, &associations_path).await == json!([]));
    assert!(account.get(app, &other_associations).await == json!([]));
    assert_eq!(provider.requests().len(), 5);

    let (status, confirmed) = account
        .request(
            app,
            "POST",
            &format!("{review_path}/confirm"),
            selection.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let id = confirmed["id"].as_str().unwrap().to_owned();
    assert!(Uuid::parse_str(&id).is_ok());
    let expected = ArtistAssociation {
        id: id.clone(),
        account_id: account.id.clone(),
        library_id: context.library_id.clone(),
        recording_id: imported.recording.clone(),
        artist_id: imported.artist.clone(),
        source_id: None,
        observation_id: imported.observation.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: selected,
        provider_artist_id: chosen,
        evidence: ArtistAssociationEvidence::UserConfirmed,
        credit_evidence: None,
        revision: 1,
    };
    assert!(confirmed == json!(expected));
    let (status, duplicate) = account
        .request(app, "POST", &format!("{review_path}/confirm"), selection)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(duplicate == confirmed);
    assert!(account.get(app, &associations_path).await == json!([expected]));
    assert!(account.get(app, &other_associations).await == json!([]));
    assert_eq!(
        provider.requests().len(),
        5,
        "Confirmation must require no provider HTTP"
    );
    let profile = account
        .get(app, &format!("{associations_path}/{id}/profile"))
        .await;
    assert!(
        profile
            == json!(ArtistProfileAvailability::Available {
                association_id: id,
                profile: catalog::expected_profile(),
            })
    );
    assert!(
        imported.snapshot(app, account).await == before,
        "HTTP artist review must preserve private pins, duplicate entries, likes, progress, observations, source/download absence and collection events"
    );
    assert_eq!(fixture::origin_counts(origin).await, origin_before);
    let observed = provider.requests();
    assert_eq!(observed.len(), 7);
    assert!(observed[0].method == Method::GET && observed[0].path == "/prefix/v1/manifest");
    assert!(
        observed[1].method == Method::GET
            && observed[1].path == "/prefix/v1/catalog/discovery/capabilities"
            && observed[1].body.is_null()
    );
    assert!(
        observed[2].method == Method::POST
            && observed[2].path == "/prefix/v1/catalog/artists/search"
            && observed[2].body == json!(search),
        "Only the SDK query envelope may be forwarded to artist discovery"
    );
    let (pairs, remainder) = observed[3..].as_chunks::<2>();
    assert!(remainder.is_empty());
    for pair in pairs {
        assert!(
            pair[0].method == Method::GET
                && pair[0].path == "/prefix/v1/catalog/capabilities"
                && pair[0].body.is_null()
        );
        assert!(
            pair[1].method == Method::POST
                && pair[1].path == "/prefix/v1/catalog/artists/profile"
                && pair[1].body == json!({"artist_id": catalog::SECOND_ARTIST})
        );
    }
    for request in observed {
        assert!(
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                == Some("Bearer private-artist-discovery-service-token")
        );
        assert!(request.headers.get("cookie").is_none());
    }
    assert_eq!(provider.unexpected(), 0);
}
