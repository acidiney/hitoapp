use crate::{
    album_editions::{Prepared, prepare},
    artist_review_fixture::{self as fixture, Account},
    support,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use serde_json::json;
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};

#[tokio::test]
async fn a_reviewed_artists_catalog_track_finds_and_saves_approved_audio_over_http() {
    let (_directory, app) = support::app().await;
    let account = Account::setup(&app).await;
    let mut catalog = AlbumProvider::start().await;
    let audio = crate::provider_fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        AssertUnwindSafe(async {
            let Prepared {
                first, association, ..
            } = prepare(&app, &account, &catalog).await;
            let path = format!(
                "/api/v1/recordings/{first}/artist-associations/{association}/edition-track-audio"
            );
            let body = json!({
                "release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID,
                "occurrence_id": "occurrence:album/2/2"
            });
            let before = account.get(&app, "/api/v1/recordings").await["total"].clone();
            let list_path = format!(
                "/api/v1/recordings/{first}/artist-associations/{association}/edition-track-list"
            );
            let list_body = json!({"release_group_id": provider::GROUP_ID, "edition_id": provider::EDITION_ID});
            let (status, _, unlinked) =
                fixture::exchange(&app, "POST", &list_path, list_body.clone(), account.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(unlinked["linked"], json!([]));

            let mut anonymous = account.headers();
            anonymous.remove("cookie");
            let (status, _, _) =
                fixture::exchange(&app, "POST", &path, body.clone(), anonymous).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            let mut unguarded = account.headers();
            unguarded.remove("x-hito-request");
            let (status, _, _) =
                fixture::exchange(&app, "POST", &path, body.clone(), unguarded).await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            let mut unknown = body.clone();
            unknown["title"] = json!("Bonus catalog title");
            let (status, _, _) =
                fixture::exchange(&app, "POST", &path, unknown, account.headers()).await;
            assert!(status.is_client_error());

            let (status, _, value) =
                fixture::exchange(&app, "POST", &path, body.clone(), account.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(value, json!({"status": "no_providers"}));

            audio
                .reply(
                    "v1/manifest",
                    json!({"id": "org.hito.audio-fixture", "name": "Audio fixture", "protocol_version": 1,
                        "capabilities": ["search", "sources", "metadata"]}),
                )
                .await;
            account.connect(&app, &audio.url, "Audio fixture", None).await;
            let detail = json!({
                "id": "audio/bonus", "title": "Bonus catalog title", "artist": provider::CREDIT,
                "duration_ms": 1000, "release": null, "disc_number": null, "track_number": null,
                "revision": "audio-r1"
            });
            audio
                .reply("v1/search", json!({"items": [detail.clone()], "next_cursor": null}))
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
            let (status, _, saved) =
                fixture::exchange(&app, "POST", &path, body.clone(), account.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(saved["status"], "saved");
            assert_eq!(saved["recording"]["title"], "Bonus catalog title");
            assert_eq!(
                account.get(&app, "/api/v1/recordings").await["total"],
                json!(before.as_i64().unwrap() + 1)
            );
            assert!(audio.requests("v1/audio").await.is_empty());
            let (status, headers, linked) =
                fixture::exchange(&app, "POST", &list_path, list_body.clone(), account.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers["cache-control"], "private, no-store");
            assert_eq!(linked["linked"].as_array().unwrap().len(), 1);
            assert_eq!(linked["linked"][0]["occurrence_id"], "occurrence:album/2/2");
            assert_eq!(linked["linked"][0]["recording"]["id"], saved["recording"]["id"]);
            let mut anonymous = account.headers();
            anonymous.remove("cookie");
            let (status, _, hidden) =
                fixture::exchange(&app, "POST", &list_path, list_body, anonymous).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(hidden.get("linked").is_none());

            let (status, _) = account
                .request(
                    &app,
                    "PUT",
                    "/api/v1/metadata/automation",
                    json!({"automatic_service_search": false}),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            let searches = audio.requests("v1/search").await.len();
            let (status, _, off) =
                fixture::exchange(&app, "POST", &path, body.clone(), account.headers()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(off, json!({"status": "search_off"}));
            assert_eq!(audio.requests("v1/search").await.len(), searches);

            let contract = serde_json::to_value(hito_server::openapi()).unwrap();
            let endpoint = &contract["paths"]
                ["/api/v1/recordings/{id}/artist-associations/{association}/edition-track-audio"]
                ["post"];
            assert_eq!(endpoint["operationId"], "recording_artist_edition_track_audio");
            assert_eq!(
                endpoint["requestBody"]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/FindArtistTrackAudio"
            );
            assert_eq!(
                endpoint["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
                "#/components/schemas/AlbumTrackAudio"
            );
        })
        .catch_unwind(),
    )
    .await;
    audio.shutdown().await;
    let joined = catalog.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The artist track audio workflow must finish within its deadline"),
    }
    assert!(joined);
}
