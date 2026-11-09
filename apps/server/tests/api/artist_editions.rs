use hito_core::test_support::artist_catalog_editions_service as catalog;

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
    ArtistReviewContext, EditionTrackListAvailability, MetadataProvider, ReadEditionTrackList,
    ReadReleaseGroupEditions, ReleaseGroupEditionsAvailability,
};
use hito_plugin_sdk::{CatalogEditionTrackList, EditionTrackListRequest};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn an_authenticated_account_browses_and_explicitly_reads_its_chosen_edition() {
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
        Err(_) => panic!("The authenticated edition workflow must finish within its deadline"),
    }
    assert!(joined, "The owned edition provider must join shutdown");
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
        .connect(app, &provider.url, "Owned HTTP editions", None)
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation["id"].as_str().unwrap().into(),
    };
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    let (status, _) = account
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
    let choice = json!({"context": context, "provider_artist_id": catalog::ARTIST_ID});
    let preview = accepted(app, account, &format!("{review}/preview"), choice.clone()).await;
    assert!(
        preview["status"] == "available"
            && preview["profile"]["artist"]["id"] == catalog::ARTIST_ID
    );
    let confirmed = accepted(app, account, &format!("{review}/confirm"), choice).await;
    let association = confirmed["id"].as_str().unwrap();
    assert!(Uuid::parse_str(association).is_ok());
    assert!(
        confirmed["account_id"] == account.id
            && confirmed["recording_id"] == imported.recording
            && confirmed["artist_id"] == imported.artist
            && confirmed["observation_id"] == imported.observation
            && confirmed["source_id"].is_null()
            && confirmed["provider"] == json!(selected)
            && confirmed["provider_artist_id"] == catalog::ARTIST_ID
    );
    let before = imported.snapshot(app, account).await;
    let base = format!("{associations}/{association}");
    let groups = accepted(
        app,
        account,
        &format!("{base}/discography"),
        json!({"limit": 1, "continuation": null}),
    )
    .await;
    assert_eq!(groups["association_id"], association);
    let group = groups["page"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["id"] == catalog::GROUP_ID)
        .unwrap();
    let group_id = group["id"].as_str().unwrap();
    assert_eq!(group["title"], "Owned album");
    assert!(
        provider
            .requests()
            .iter()
            .all(|request| !request.path.contains("/editions"))
    );
    provider.enable_edition_listing();
    let list_path = format!("{base}/release-group-editions");
    let tracks_path = format!("{base}/edition-track-list");
    let (status, headers, first) = fixture::exchange(
        app,
        "POST",
        &list_path,
        page_body(group_id, None),
        account.headers(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "The authenticated edition-list route must exist"
    );
    private(&headers);
    let continuation = first["continuation"].as_str().unwrap().to_owned();
    assert!(continuation.is_ascii() && !continuation.is_empty() && continuation.len() <= 8192);
    assert_ne!(continuation, catalog::PROVIDER_CURSOR);
    assert!(
        first
            == json!(ReleaseGroupEditionsAvailability::Available {
                association_id: association.into(),
                page: catalog::expected_editions(false),
                continuation: Some(continuation.clone()),
            })
    );
    let (status, headers, last) = fixture::exchange(
        app,
        "POST",
        &list_path,
        page_body(group_id, Some(continuation)),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert!(
        last == json!(ReleaseGroupEditionsAvailability::Available {
            association_id: association.into(),
            page: catalog::expected_editions(true),
            continuation: None,
        })
    );
    assert_eq!(
        first["page"]["items"][0]["title"],
        last["page"]["items"][0]["title"]
    );
    assert_ne!(
        first["page"]["items"][0]["id"],
        last["page"]["items"][0]["id"]
    );
    let selected_edition = last["page"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edition| edition["id"] == catalog::BONUS_ID)
        .unwrap();
    let selected_id = selected_edition["id"].as_str().unwrap();
    assert!(
        provider
            .requests()
            .iter()
            .all(|request| request.path != "/prefix/v1/catalog/editions/track-list"),
        "Listing must not eagerly fetch any track list"
    );
    let track_body = track_body(group_id, selected_id);
    let before_anonymous = provider.requests();
    for (path, body) in [
        (&list_path, page_body(group_id, None)),
        (&tracks_path, track_body.clone()),
    ] {
        let mut anonymous = account.headers();
        anonymous.remove("cookie");
        let (status, headers, _) = fixture::exchange(app, "POST", path, body, anonymous).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        private(&headers);
    }
    assert_eq!(
        provider.requests(),
        before_anonymous,
        "Anonymous reads must fail before provider I/O"
    );
    let (status, headers, tracks) =
        fixture::exchange(app, "POST", &tracks_path, track_body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert!(
        tracks
            == json!(EditionTrackListAvailability::Available {
                association_id: association.into(),
                edition: catalog::expected_track_list(),
                linked: Vec::new(),
            })
    );
    let edition: CatalogEditionTrackList =
        serde_json::from_value(tracks["edition"].clone()).unwrap();
    edition
        .validate_for(&EditionTrackListRequest {
            release_group_id: group_id.into(),
            edition_id: selected_id.into(),
        })
        .unwrap();
    assert_ne!(edition.media[0].tracks[0].id, edition.media[1].tracks[0].id);
    assert_eq!(
        edition.media[0].tracks[0].recording_id,
        edition.media[1].tracks[0].recording_id
    );
    let requests = provider.requests();
    let pages = requests
        .iter()
        .filter(|request| request.path == "/prefix/v1/catalog/release-groups/editions")
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 2);
    for (request, cursor) in pages
        .iter()
        .zip([Value::Null, json!(catalog::PROVIDER_CURSOR)])
    {
        assert!(request.method == Method::POST);
        assert_eq!(
            request.body,
            json!({"release_group_id": group_id, "limit": 1, "cursor": cursor, "snapshot": null})
        );
    }
    let track_requests = requests
        .iter()
        .filter(|request| request.path == "/prefix/v1/catalog/editions/track-list")
        .collect::<Vec<_>>();
    assert_eq!(track_requests.len(), 1);
    assert!(track_requests[0].method == Method::POST);
    assert_eq!(
        track_requests[0].body,
        json!({"release_group_id": group_id, "edition_id": selected_id})
    );
    assert!(imported.snapshot(app, account).await == before);
    assert_eq!(account.get(app, &associations).await, json!([confirmed]));
    assert_eq!(
        account
            .get(
                app,
                &format!(
                    "/api/v1/recordings/{}/artist-associations",
                    imported.other_recording
                )
            )
            .await,
        json!([])
    );
    assert_eq!(fixture::origin_counts(origin).await, origin_before);
    assert_eq!(provider.unexpected(), 0);
    assert!(
        requests.iter().all(|request| matches!(
            request.path.as_str(),
            "/prefix/v1/manifest"
                | "/prefix/v1/catalog/capabilities"
                | "/prefix/v1/catalog/artists/profile"
                | "/prefix/v1/catalog/artists/discography"
                | "/prefix/v1/catalog/editions/capabilities"
                | "/prefix/v1/catalog/release-groups/editions"
                | "/prefix/v1/catalog/editions/track-list"
        )),
        "Edition browsing must not request sources, audio, downloads or adoption"
    );
    assert_contract();
}

fn page_body(group: &str, continuation: Option<String>) -> Value {
    let request = ReadReleaseGroupEditions {
        release_group_id: group.into(),
        limit: 1,
        continuation,
    };
    json!({"release_group_id": request.release_group_id, "limit": request.limit, "continuation": request.continuation})
}

fn track_body(group: &str, edition: &str) -> Value {
    let request = ReadEditionTrackList {
        release_group_id: group.into(),
        edition_id: edition.into(),
    };
    json!({"release_group_id": request.release_group_id, "edition_id": request.edition_id})
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
    let correlation = headers.get("x-request-id").unwrap().to_str().unwrap();
    assert!(Uuid::parse_str(correlation).is_ok());
}

async fn accepted(app: &Router, account: &Account, path: &str, body: Value) -> Value {
    let (status, headers, value) =
        fixture::exchange(app, "POST", path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    value
}

fn assert_contract() {
    let contract = serde_json::to_value(hito_server::openapi()).unwrap();
    let prefix = "/api/v1/recordings/{id}/artist-associations/{association}";
    for (suffix, operation, request, response) in [
        (
            "release-group-editions",
            "recording_artist_release_group_editions",
            "ReadReleaseGroupEditions",
            "ReleaseGroupEditionsAvailability",
        ),
        (
            "edition-track-list",
            "recording_artist_edition_track_list",
            "ReadEditionTrackList",
            "EditionTrackListAvailability",
        ),
    ] {
        let endpoint = &contract["paths"][format!("{prefix}/{suffix}")]["post"];
        assert_eq!(endpoint["operationId"], operation);
        assert_eq!(
            endpoint["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            format!("#/components/schemas/{request}")
        );
        assert_eq!(
            endpoint["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
            format!("#/components/schemas/{response}")
        );
        assert_eq!(
            endpoint["security"],
            json!([{"session": []}, {"bearer": []}])
        );
        assert!(endpoint["responses"]["200"]["headers"]["Cache-Control"].is_object());
    }
    for schema in [
        "CatalogEditionSummary",
        "ReleaseGroupEditionsPage",
        "CatalogEditionTrackList",
        "CatalogEditionMedium",
        "CatalogTrackOccurrence",
        "CatalogTrackKind",
    ] {
        assert!(contract["components"]["schemas"][schema].is_object());
    }
}
