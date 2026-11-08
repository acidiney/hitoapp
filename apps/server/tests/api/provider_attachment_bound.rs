use super::{
    provider_attachment::connect,
    provider_fixture::ProviderServer,
    support::{admin, call, services},
};
use axum::http::{HeaderMap, StatusCode};
use serde_json::json;

#[tokio::test]
async fn attaching_a_source_that_belongs_to_another_track_names_that_track() {
    let directory = tempfile::tempdir().unwrap();
    let hito = services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito);
    let owner = admin(&app).await;
    let mut installations = Vec::new();
    let mut recordings = Vec::new();
    for id in ["first", "second"] {
        let provider = ProviderServer::start().await;
        provider
            .reply(
                "v1/recordings/detail",
                json!({"id": id, "title": format!("{id} title"), "artist": "Artist", "duration_ms": 90_000}),
            )
            .await;
        provider
            .reply(
                "v1/sources",
                json!({"items": [{"id": format!("{id}-audio"), "label": "Owned audio", "ready": true}]}),
            )
            .await;
        provider
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([(
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                )]),
                vec![0; 64],
            )
            .await;
        let installation = connect(&app, &owner, &provider).await;
        let (status, adopted) = call(
            &app,
            "POST",
            &format!(
                "/api/v1/plugins/{}/recordings",
                installation["id"].as_str().unwrap()
            ),
            json!({"installation_revision": installation["revision"], "provider_recording_id": id}),
            Some(&owner),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        recordings.push(adopted["id"].as_str().unwrap().to_owned());
        installations.push((installation, provider));
    }
    let candidate = json!({
        "installation_id": installations[1].0["id"],
        "installation_revision": installations[1].0["revision"],
        "provider_recording_id": "second"
    });
    let (status, body) = call(
        &app,
        "POST",
        &format!("/api/v1/recordings/{}/sources/review", recordings[0]),
        candidate,
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "already_bound");
    assert_eq!(body["recording_id"], recordings[1]);
    assert!(!body["error"].as_str().unwrap().contains("provider"));
}
