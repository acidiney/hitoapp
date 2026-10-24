use crate::{
    artist_review_fixture::{self as fixture, Account},
    support,
};
use axum::{Router, http::StatusCode};
use futures_util::FutureExt;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn an_account_compares_a_selected_edition_with_its_album_without_saving_catalog_tracks() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let mut server = AlbumProvider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        AssertUnwindSafe(exercise(&app, &account, &server)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The album edition workflow must finish within its deadline"),
    }
    assert!(joined, "The owned album provider must join shutdown");
}

async fn exercise(app: &Router, account: &Account, server: &AlbumProvider) {
    let installation = account
        .connect(app, &server.url, "Owned album editions", None)
        .await;
    let installation_id = installation["id"].as_str().unwrap();
    let mut saved = Vec::new();
    for id in [provider::FIRST, provider::SECOND, provider::EXTRA] {
        let (status, recording) = account
            .request(
                app,
                "POST",
                &format!("/api/v1/plugins/{installation_id}/recordings"),
                json!({"installation_revision": installation["revision"], "provider_recording_id": id}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        saved.push(recording);
    }
    let release = saved[0]["release_id"].as_str().unwrap().to_owned();
    let first = saved[0]["id"].as_str().unwrap().to_owned();
    let selected = json!({"kind": "http_plugin", "installation_id": installation_id});
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    let (status, _) = account
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({"revision": settings["revision"], "enabled": true, "providers": [selected]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let album = format!("/api/v1/releases/{release}");
    assert!(account.get(app, &album).await["catalog_anchor"].is_null());

    let review = format!("/api/v1/recordings/{first}/artist-review");
    let context = accepted(
        app,
        account,
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let choice = json!({"context": context, "provider_artist_id": provider::ARTIST_ID});
    accepted(app, account, &format!("{review}/preview"), choice.clone()).await;
    let association = accepted(app, account, &format!("{review}/confirm"), choice).await;
    let association = association["id"].as_str().unwrap().to_owned();

    let detail = account.get(app, &album).await;
    assert_eq!(
        detail["catalog_anchor"],
        json!({"recording_id": first, "association_id": association})
    );

    let path = format!("/api/v1/albums/{release}/edition-track-list");
    let body = json!({
        "recording_id": first, "association_id": association,
        "release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID
    });
    let details_before = server.requests_to("v1/recordings/detail").len();
    let requests_before = server.requests().len();

    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let (status, _, _) = fixture::exchange(app, "POST", &path, body.clone(), anonymous).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut unguarded = account.headers();
    unguarded.remove("x-hito-request");
    let (status, _, _) = fixture::exchange(app, "POST", &path, body.clone(), unguarded).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut unknown = body.clone();
    unknown["extra"] = json!(true);
    let (status, _, _) = fixture::exchange(app, "POST", &path, unknown, account.headers()).await;
    assert!(status.is_client_error());
    let (status, _, _) = fixture::exchange(
        app,
        "POST",
        &format!("/api/v1/albums/{}/edition-track-list", Uuid::new_v4()),
        body.clone(),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        server.requests().len(),
        requests_before,
        "Rejected requests fail before provider I/O"
    );

    let (status, headers, merged) =
        fixture::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
    assert!(Uuid::parse_str(headers.get("x-request-id").unwrap().to_str().unwrap()).is_ok());
    assert_eq!(merged["status"], "available");
    assert_eq!(merged["association_id"], association);
    assert_eq!(merged["is_album_release"], true);
    let linked: Vec<(&str, &str)> = merged["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["occurrence_id"].as_str().unwrap(),
                m["recording"]["id"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        linked,
        [
            ("occurrence:album/1/1", first.as_str()),
            ("occurrence:album/1/2", saved[1]["id"].as_str().unwrap()),
            ("occurrence:album/2/1", first.as_str()),
        ]
    );
    assert_eq!(merged["attached_occurrences"], json!([]));
    assert_eq!(merged["library_only"].as_array().unwrap().len(), 1);
    assert_eq!(merged["library_only"][0]["id"], saved[2]["id"]);
    assert_eq!(merged["adoption"]["installation_id"], installation_id);
    assert_eq!(
        merged["adoption"]["installation_revision"],
        installation["revision"]
    );
    assert_eq!(
        server.requests_to("v1/recordings/detail").len(),
        details_before,
        "Reading must not save catalog-only tracks"
    );
    assert!(server.requests_to("v1/sources").is_empty());
    assert!(server.requests_to("v1/audio").is_empty());

    server.set_outage(true);
    let (status, _, outage) = fixture::exchange(app, "POST", &path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outage, json!({"status": "unavailable"}));
    assert_eq!(account.get(app, &album).await["tracks"]["total"], 3);
    assert_eq!(server.unexpected(), 0);
    assert_contract();
}

async fn accepted(app: &Router, account: &Account, path: &str, body: Value) -> Value {
    let (status, _, value) = fixture::exchange(app, "POST", path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    value
}

fn assert_contract() {
    let contract = serde_json::to_value(hito_server::openapi()).unwrap();
    let endpoint = &contract["paths"]["/api/v1/albums/{id}/edition-track-list"]["post"];
    assert_eq!(endpoint["operationId"], "album_edition_track_list");
    assert_eq!(
        endpoint["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ReadAlbumEditionTrackList"
    );
    assert_eq!(
        endpoint["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AlbumEditionTrackListAvailability"
    );
    assert_eq!(
        endpoint["security"],
        json!([{"session": []}, {"bearer": []}])
    );
    assert!(endpoint["responses"]["200"]["headers"]["Cache-Control"].is_object());
}
