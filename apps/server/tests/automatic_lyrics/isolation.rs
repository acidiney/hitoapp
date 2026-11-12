use super::setup::{Prepared, commit_metadata_for};
use crate::{account, lrclib, source_provider};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hito_core::{AutomaticLyricsLookup, Error};
use serde_json::{Value, json};
use std::time::Duration;
use tower::ServiceExt;

#[tokio::test]
async fn automatic_work_shares_manual_lyric_quota_and_leaves_ordinary_source_refresh_and_exact_audio_independent()
 {
    let prepared = Prepared::new().await;
    let base = prepared.path().replace("/automatic", "");
    let candidate = json!({"installation_id":prepared.installation["id"],"installation_revision":prepared.installation["revision"],"provider_recording_id":"owned:automatic"});
    let (status, review) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{base}/identities/review"),
            candidate.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, identity) = prepared
        .library
        .account
        .request(
            &prepared.library.app,
            "POST",
            &format!("{base}/identities"),
            json!({"candidate":candidate,"review_stamp":review["review_stamp"]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let mut source = source_provider::Provider::start("automatic-lyric-isolation", 0).await;
    let installation = prepared
        .library
        .account
        .connect(
            &prepared.library.app,
            &source.url,
            "Owned ordinary audio",
            None,
        )
        .await;
    let (status, adopted) = prepared.library.account.request(&prepared.library.app, "POST", &format!("/api/v1/plugins/{}/recordings", installation["id"].as_str().unwrap()), json!({"installation_revision":installation["revision"],"provider_recording_id":source.recording_id()})).await;
    assert_eq!(status, StatusCode::OK);
    let sources = format!(
        "/api/v1/recordings/{}/sources",
        adopted["id"].as_str().unwrap()
    );
    let before = prepared.library.snapshot().await;
    for hop in ["search", "metadata"] {
        source.replace_unavailable();
        source.make_ready();
        let gate = prepared.provider.hold(hop);
        let app = prepared.library.app.clone();
        let path = prepared.path();
        let headers = prepared.library.account.headers();
        let task = tokio::spawn(async move {
            account::exchange(&app, "POST", &path, json!({}), headers).await
        });
        tokio::time::timeout(Duration::from_secs(2), gate.entered())
            .await
            .unwrap();
        for (path, body) in [
            (
                format!("{base}/candidates"),
                json!({"installation_id":prepared.installation["id"],"installation_revision":prepared.installation["revision"],"query":"Owned explicit manual query"}),
            ),
            (format!("{base}/identities/review"), candidate.clone()),
            (
                format!("{base}/identities/read"),
                json!({"selector":identity["selector"],"installation_revision":prepared.installation["revision"]}),
            ),
        ] {
            let (status, _) = prepared
                .library
                .account
                .request(&prepared.library.app, "POST", &path, body)
                .await;
            assert_eq!(
                status,
                StatusCode::CONFLICT,
                "All lyric workflows must share the same dedicated account quota"
            );
        }
        let (status, refreshed) = tokio::time::timeout(
            Duration::from_secs(2),
            prepared.library.account.request(
                &prepared.library.app,
                "POST",
                &format!("{sources}/refresh"),
                json!({"purpose":"playback"}),
            ),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::OK);
        let ready = refreshed
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["ready"] == true)
            .unwrap();
        let mut request = Request::get(format!(
            "/api/v1/sources/{}/audio",
            ready["id"].as_str().unwrap()
        ))
        .body(Body::empty())
        .unwrap();
        *request.headers_mut() = prepared.library.account.headers();
        request
            .headers_mut()
            .insert("range", "bytes=44-107".parse().unwrap());
        tokio::time::timeout(Duration::from_secs(2), async {
            let response = prepared.library.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                response.headers()["content-range"],
                format!("bytes 44-107/{}", source.audio().len())
            );
            assert_eq!(
                to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap()
                    .as_ref(),
                &source.audio()[44..108]
            );
        })
        .await
        .expect("Ordinary audio must complete while automatic lyric work remains held");
        assert!(!gate.released() && !task.is_finished());
        prepared.provider.release();
        let (status, _, response) = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["status"], "ready_v2");
    }
    assert_eq!(
        prepared
            .library
            .account
            .get(&prepared.library.app, &format!("{base}/identities"))
            .await["items"],
        json!([identity])
    );
    assert_eq!(prepared.library.snapshot().await, before);
    assert_eq!(source.audio_requests(), 2);
    assert_eq!(source.unexpected(), 0);
    assert!(source.finish().await);
    prepared.finish().await;
}

#[tokio::test]
async fn four_accounts_can_hold_automatic_work_but_a_fifth_is_rejected_before_upstream_and_recovers_after_cancel()
 {
    let prepared = Prepared::new().await;
    let library = &prepared.library;
    let libraries = library.account.get(&library.app, "/api/v1/libraries").await;
    let shared = libraries
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["personal"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let mut actors = vec![prepared.actor().await];
    let mut installations = vec![prepared.installation.clone()];
    for index in 1..=4 {
        let username = format!("automatic-capacity-{index}");
        let (status, user) = library
            .account
            .request(
                &library.app,
                "POST",
                "/api/v1/users",
                json!({"username":username,"password":"owned automatic capacity password"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = library
            .account
            .request(
                &library.app,
                "PUT",
                &format!(
                    "/api/v1/libraries/{shared}/members/{}",
                    user["id"].as_str().unwrap()
                ),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let peer =
            account::Account::login(&library.app, &username, "owned automatic capacity password")
                .await;
        commit_metadata_for(library, &peer, lrclib::TITLE, lrclib::ARTIST).await;
        let (status, installation) = peer.request(&library.app, "POST", "/api/v1/plugins", json!({"service_id":prepared.installation["service_id"],"service_revision":1,"bearer_token":null})).await;
        assert_eq!(status, StatusCode::CREATED);
        let headers = peer.headers();
        let session = headers["cookie"]
            .to_str()
            .unwrap()
            .strip_prefix("hito_session=")
            .unwrap();
        actors.push(
            library
                .services
                .identity
                .authenticate(session)
                .await
                .unwrap(),
        );
        installations.push(installation);
    }
    let before = library.snapshot().await;
    let gate = prepared.provider.hold("search");
    let mut tasks = Vec::new();
    for actor in actors.iter().take(4) {
        let request_actor = actor.clone();
        let catalog = library.services.catalog.clone();
        let recording = library.recording.clone();
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            catalog
                .automatic_recording_lyrics_until(&request_actor, &recording, async {
                    let _ = cancelled.await;
                })
                .await
        });
        tasks.push((cancel, task));
    }
    for _ in 0..4 {
        tokio::time::timeout(Duration::from_secs(3), gate.entered())
            .await
            .unwrap();
    }
    let fifth = tokio::time::timeout(
        Duration::from_secs(2),
        library.services.catalog.automatic_recording_lyrics_until(
            &actors[4],
            &library.recording,
            std::future::pending(),
        ),
    )
    .await
    .unwrap();
    assert!(matches!(fifth, Err(Error::Conflict(_))));
    assert_eq!(
        prepared
            .provider
            .calls()
            .into_iter()
            .filter(|operation| *operation == "search")
            .count(),
        4
    );
    for (cancel, task) in tasks {
        cancel.send(()).unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            Ok(AutomaticLyricsLookup::Cancelled)
        ));
    }
    for _ in 0..4 {
        tokio::time::timeout(Duration::from_secs(2), gate.retired())
            .await
            .unwrap();
    }
    assert!(!gate.released());
    prepared.provider.release();
    let fifth = library
        .services
        .catalog
        .automatic_recording_lyrics_until(&actors[4], &library.recording, std::future::pending())
        .await
        .unwrap();
    let value = serde_json::to_value(fifth).unwrap();
    assert_eq!(value["status"], "ready_v2");
    assert_eq!(value["installation_id"], installations[4]["id"]);
    assert_eq!(library.snapshot().await, before);
    prepared.finish().await;
}
