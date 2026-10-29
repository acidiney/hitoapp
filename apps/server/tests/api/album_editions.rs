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

pub(crate) struct Prepared {
    pub installation: Value,
    pub saved: Vec<Value>,
    pub release: String,
    pub first: String,
    pub association: String,
}

pub(crate) async fn prepare(app: &Router, account: &Account, server: &AlbumProvider) -> Prepared {
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
    Prepared {
        installation,
        saved,
        release,
        first,
        association,
    }
}

async fn exercise(app: &Router, account: &Account, server: &AlbumProvider) {
    let Prepared {
        installation,
        saved,
        release,
        first,
        association,
    } = prepare(app, account, server).await;
    let installation_id = installation["id"].as_str().unwrap();
    let album = format!("/api/v1/releases/{release}");
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

#[tokio::test]
async fn an_account_remembers_the_edition_it_chose_for_an_album_until_it_clears_it() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let mut server = AlbumProvider::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        AssertUnwindSafe(remember(&app, &account, &server)).catch_unwind(),
    )
    .await;
    let joined = server.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The saved edition workflow must finish within its deadline"),
    }
    assert!(joined, "The owned album provider must join shutdown");
}

async fn remember(app: &Router, account: &Account, server: &AlbumProvider) {
    let Prepared {
        release,
        first,
        association,
        ..
    } = prepare(app, account, server).await;
    let album = format!("/api/v1/releases/{release}");
    let choose = format!("/api/v1/albums/{release}/edition");
    let clear = format!("/api/v1/albums/{release}/edition/clear");
    let read = format!("/api/v1/albums/{release}/edition/track-list");
    assert_eq!(
        account.get(app, &album).await["saved_edition"],
        json!({"revision": 0, "state": {"status": "unset"}})
    );
    let request_id = Uuid::new_v4().to_string();
    let body = json!({
        "request_id": request_id, "expected_revision": 0,
        "recording_id": first, "association_id": association,
        "release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID
    });
    let requests_before = server.requests().len();

    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let (status, _, _) = fixture::exchange(app, "PUT", &choose, body.clone(), anonymous).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut unguarded = account.headers();
    unguarded.remove("x-hito-request");
    let (status, _, _) = fixture::exchange(app, "PUT", &choose, body.clone(), unguarded).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut unknown = body.clone();
    unknown["extra"] = json!(true);
    let (status, _, _) = fixture::exchange(app, "PUT", &choose, unknown, account.headers()).await;
    assert!(status.is_client_error());
    let (status, _, _) = fixture::exchange(
        app,
        "PUT",
        &format!("/api/v1/albums/{}/edition", Uuid::new_v4()),
        body.clone(),
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = fixture::exchange(
        app,
        "POST",
        &format!("/api/v1/albums/{}/edition/track-list", Uuid::new_v4()),
        Value::Null,
        account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, needs) =
        fixture::exchange(app, "POST", &read, Value::Null, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(needs, json!({"status": "needs_choice"}));
    assert_eq!(server.requests().len(), requests_before);

    let (status, _, receipt) =
        fixture::exchange(app, "PUT", &choose, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    let expected = json!({
        "request_id": request_id, "release_id": release, "revision": 1,
        "edition": {
            "recording_id": first, "association_id": association,
            "release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID
        }
    });
    assert_eq!(receipt, expected);
    let (status, _, replay) =
        fixture::exchange(app, "PUT", &choose, body.clone(), account.headers()).await;
    assert_eq!((status, replay), (StatusCode::OK, expected.clone()));
    let mut reused = body.clone();
    reused["edition_id"] = json!(provider::STANDARD_ID);
    let (status, _, _) = fixture::exchange(app, "PUT", &choose, reused, account.headers()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let mut stale = body.clone();
    stale["request_id"] = json!(Uuid::new_v4().to_string());
    let (status, _, _) = fixture::exchange(app, "PUT", &choose, stale, account.headers()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        server.requests().len(),
        requests_before,
        "Saving never calls the provider"
    );

    assert_eq!(
        account.get(app, &album).await["saved_edition"],
        json!({"revision": 1, "state": {"status": "ready", "edition": expected["edition"]}})
    );
    let (status, headers, merged) =
        fixture::exchange(app, "POST", &read, Value::Null, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("cache-control").unwrap(), "private, no-store");
    assert_eq!(merged["status"], "available");
    assert_eq!(merged["edition"]["id"], provider::EDITION_ID);
    assert_eq!(merged["matches"].as_array().unwrap().len(), 3);
    assert_eq!(
        server.requests_to("v1/catalog/editions/track-list").len(),
        1
    );

    let clear_body = json!({"request_id": Uuid::new_v4().to_string(), "expected_revision": 1});
    let (status, _, cleared) =
        fixture::exchange(app, "POST", &clear, clear_body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["revision"], 2);
    assert!(cleared["edition"].is_null());
    let (status, _, replay) =
        fixture::exchange(app, "POST", &clear, clear_body, account.headers()).await;
    assert_eq!((status, replay), (StatusCode::OK, cleared));
    assert_eq!(
        account.get(app, &album).await["saved_edition"],
        json!({"revision": 2, "state": {"status": "unset"}})
    );
    let (_, _, after) = fixture::exchange(app, "POST", &read, Value::Null, account.headers()).await;
    assert_eq!(after, json!({"status": "needs_choice"}));
    assert_eq!(server.unexpected(), 0);
    assert_saved_contract();
}

fn assert_saved_contract() {
    let contract = serde_json::to_value(hito_server::openapi()).unwrap();
    let choose = &contract["paths"]["/api/v1/albums/{id}/edition"]["put"];
    assert_eq!(choose["operationId"], "choose_album_edition");
    assert_eq!(
        choose["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ChooseAlbumEdition"
    );
    assert_eq!(
        choose["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AlbumEditionChoiceReceipt"
    );
    let clear = &contract["paths"]["/api/v1/albums/{id}/edition/clear"]["post"];
    assert_eq!(clear["operationId"], "clear_album_edition");
    let read = &contract["paths"]["/api/v1/albums/{id}/edition/track-list"]["post"];
    assert_eq!(read["operationId"], "saved_album_edition_track_list");
    assert!(read["responses"]["200"]["headers"]["Cache-Control"].is_object());
    for endpoint in [choose, clear, read] {
        assert_eq!(
            endpoint["security"],
            json!([{"session": []}, {"bearer": []}])
        );
    }
}

#[tokio::test]
async fn a_catalog_only_track_finds_approved_audio_over_http_without_saving_anything() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let mut catalog = AlbumProvider::start().await;
    let audio = crate::provider_fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        AssertUnwindSafe(find_audio(&app, &account, &catalog, &audio)).catch_unwind(),
    )
    .await;
    audio.shutdown().await;
    let joined = catalog.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The album audio workflow must finish within its deadline"),
    }
    assert!(joined);
}

async fn find_audio(
    app: &Router,
    account: &Account,
    catalog: &AlbumProvider,
    audio: &crate::provider_fixture::ProviderServer,
) {
    let Prepared {
        release,
        first,
        association,
        ..
    } = prepare(app, account, catalog).await;
    let path = format!("/api/v1/albums/{release}/edition-track-audio");
    let body = json!({
        "recording_id": first, "association_id": association,
        "release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID,
        "occurrence_id": "occurrence:album/2/2"
    });
    let before = account.get(app, "/api/v1/recordings").await["total"].clone();

    let mut anonymous = account.headers();
    anonymous.remove("cookie");
    let (status, _, _) = fixture::exchange(app, "POST", &path, body.clone(), anonymous).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let mut unguarded = account.headers();
    unguarded.remove("x-hito-request");
    let (status, _, _) = fixture::exchange(app, "POST", &path, body.clone(), unguarded).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut unknown = body.clone();
    unknown["title"] = json!("Bonus catalog title");
    let (status, _, _) = fixture::exchange(app, "POST", &path, unknown, account.headers()).await;
    assert!(
        status.is_client_error(),
        "Caller-supplied evidence is rejected"
    );
    let (status, _, _) =
        fixture::exchange(app, "POST", &path, body.clone(), account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        accepted(app, account, &path, body.clone()).await,
        json!({"status": "no_providers"})
    );

    audio
        .reply(
            "v1/manifest",
            json!({"id": "org.hito.audio-fixture", "name": "Audio fixture", "protocol_version": 1,
                "capabilities": ["search", "sources", "metadata"]}),
        )
        .await;
    let installation = account
        .connect(app, &audio.url, "Audio fixture", None)
        .await;
    let detail = json!({
        "id": "audio/bonus", "title": "Bonus catalog title", "artist": provider::CREDIT,
        "duration_ms": 1000, "release": null, "disc_number": null, "track_number": null,
        "revision": "audio-r1"
    });
    audio
        .reply(
            "v1/search",
            json!({"items": [detail.clone()], "next_cursor": null}),
        )
        .await;
    audio.reply("v1/recordings/detail", detail.clone()).await;
    audio
        .reply(
            "v1/recordings/metadata",
            json!({"recording": detail, "musical_title": "Bonus catalog title",
                "album_title": provider::RELEASE_TITLE, "source_ids": ["audio/source"]}),
        )
        .await;
    audio
        .reply(
            "v1/sources",
            json!({"items": [{"id": "audio/source", "label": "Owned audio", "ready": true}]}),
        )
        .await;
    assert_eq!(
        accepted(app, account, &path, body.clone()).await,
        json!({
            "status": "found", "installation_id": installation["id"],
            "installation_revision": installation["revision"],
            "provider_recording_id": "audio/bonus"
        })
    );
    audio.offline().await;
    assert_eq!(
        accepted(app, account, &path, body.clone()).await,
        json!({"status": "unavailable"})
    );
    assert_eq!(
        account.get(app, "/api/v1/recordings").await["total"],
        before
    );
    assert!(audio.requests("v1/audio").await.is_empty());
    assert_eq!(catalog.unexpected(), 0);

    let contract = serde_json::to_value(hito_server::openapi()).unwrap();
    let endpoint = &contract["paths"]["/api/v1/albums/{id}/edition-track-audio"]["post"];
    assert_eq!(endpoint["operationId"], "album_edition_track_audio");
    assert_eq!(
        endpoint["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/AlbumTrackAudio"
    );
}
