use super::{
    provider_fixture::ProviderServer,
    support::{admin, call, services, write_wav},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn reviewed_http_attachment_preserves_saved_identity_and_relays_its_owned_audio() {
    let directory = tempfile::tempdir().unwrap();
    let hito = services(directory.path(), &directory.path().join("music")).await;
    let app = hito_server::router(hito);
    let owner = admin(&app).await;
    let primary = ProviderServer::start().await;
    let alternate = ProviderServer::start().await;
    let primary_audio = write_wav(&directory.path().join("owned.wav"));
    let mut alternate_audio = primary_audio.clone();
    alternate_audio[44..46].copy_from_slice(&321_i16.to_le_bytes());
    assert_ne!(primary_audio, alternate_audio);
    for (provider, id, title, bytes) in [
        (&primary, "primary", "Primary title", &primary_audio),
        (&alternate, "alternate", "Different title", &alternate_audio),
    ] {
        provider.reply("v1/recordings/detail", json!({
            "id": id, "title": title, "artist": format!("{id} artist"), "duration_ms": 90_000,
            "release": {"id": format!("{id}-album"), "title": format!("{id} album"), "artist": format!("{id} album artist")}
        })).await;
        provider
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": format!("{id}-audio"), "label": "Owned audio", "ready": true
                }]}),
            )
            .await;
        provider
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([(
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                )]),
                bytes.clone(),
            )
            .await;
    }
    let first = connect(&app, &owner, &primary).await;
    let second = connect(&app, &owner, &alternate).await;
    let (status, adopted) = call(
        &app,
        "POST",
        &format!(
            "/api/v1/plugins/{}/recordings",
            first["id"].as_str().unwrap()
        ),
        json!({"installation_revision": first["revision"], "provider_recording_id": "primary"}),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let recording = adopted["id"].as_str().unwrap();
    let playlist = Uuid::new_v4();
    let first_entry = Uuid::new_v4();
    let second_entry = Uuid::new_v4();
    let title_operation = Uuid::new_v4();
    let (status, receipt) = call(&app, "POST", "/api/v1/sync", json!({
        "device_id": "attachment-client", "operations": [
            {"id": title_operation, "action": {"kind": "recording_title_set", "recording_id": recording, "title": "Personal title"}},
            {"id": Uuid::new_v4(), "action": {"kind": "recording_artist_set", "recording_id": recording, "artist": "Personal artist"}},
            {"id": Uuid::new_v4(), "action": {"kind": "like_set", "recording_id": recording, "liked": true}},
            {"id": Uuid::new_v4(), "action": {"kind": "progress_set", "recording_id": recording, "position_ms": 4_200, "speed": 1.25}},
            {"id": Uuid::new_v4(), "action": {"kind": "playlist_create", "playlist_id": playlist, "name": "Owned playlist"}},
            {"id": Uuid::new_v4(), "action": {"kind": "entry_insert", "playlist_id": playlist, "entry_id": first_entry, "recording_id": recording, "position": 0}},
            {"id": Uuid::new_v4(), "action": {"kind": "entry_insert", "playlist_id": playlist, "entry_id": second_entry, "recording_id": recording, "position": 1}}
        ]
    }), Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 7);
    let playlist = playlist.to_string();
    let before = state(&app, &owner, recording, &playlist).await;
    assert_eq!(before["recordings"]["total"], 1);
    assert_eq!(
        before["metadata"]["title"]["operation_id"],
        title_operation.to_string()
    );
    assert_eq!(
        before["collection"]["playlist"]["entries"][0]["entry_id"],
        first_entry.to_string()
    );
    assert_eq!(
        before["collection"]["playlist"]["entries"][1]["entry_id"],
        second_entry.to_string()
    );
    let primary_source = before["sources"][0]["id"].as_str().unwrap();
    let candidate = json!({
        "installation_id": second["id"], "installation_revision": second["revision"],
        "provider_recording_id": "alternate"
    });
    let review_path = format!("/api/v1/recordings/{recording}/sources/review");
    let attach_path = format!("/api/v1/recordings/{recording}/sources/attach");
    let (status, review) = call(&app, "POST", &review_path, candidate.clone(), Some(&owner)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an owned source candidate must be reviewable through HTTP"
    );
    assert_eq!(review["candidate"]["id"], "alternate");
    assert_eq!(review["candidate"]["title"], "Different title");
    assert!(!review["review_stamp"].as_str().unwrap().is_empty());
    assert_eq!(
        state(&app, &owner, recording, &playlist).await,
        before,
        "review must not publish a binding or observation"
    );
    let attach = json!({"candidate": candidate, "review_stamp": review["review_stamp"]});
    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/users",
        json!({
            "username": "other", "password": "other attachment account password"
        }),
        Some(&owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, session) = call(
        &app,
        "POST",
        "/api/v1/login",
        json!({
            "username": "other", "password": "other attachment account password"
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let other = session["token"].as_str().unwrap();
    let calls = provider_requests(&primary, &alternate).await;
    for (path, body) in [
        (&review_path, candidate.clone()),
        (&attach_path, attach.clone()),
    ] {
        assert_eq!(
            call(&app, "POST", path, body.clone(), Some(other)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&app, "POST", path, body, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(provider_requests(&primary, &alternate).await, calls);
    assert_eq!(state(&app, &owner, recording, &playlist).await, before);
    let mut tampered = attach.clone();
    tampered["review_stamp"] = json!(format!(
        "{}-tampered",
        review["review_stamp"].as_str().unwrap()
    ));
    assert_eq!(
        call(&app, "POST", &attach_path, tampered, Some(&owner))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(state(&app, &owner, recording, &playlist).await, before);
    let (status, attached) = call(&app, "POST", &attach_path, attach, Some(&owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(attached, before["recordings"]["items"][0]);
    let after = state(&app, &owner, recording, &playlist).await;
    assert_eq!(after["recordings"], before["recordings"]);
    assert_eq!(after["collection"], before["collection"]);
    assert_eq!(after["metadata"]["title"], before["metadata"]["title"]);
    assert_eq!(after["metadata"]["artist"], before["metadata"]["artist"]);
    assert_eq!(
        after["metadata"]["selected_observation"],
        before["metadata"]["selected_observation"]
    );
    let observations = after["metadata"]["observations"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(observations.len(), 2);
    let observed = observations
        .iter()
        .find(|item| item["plugin_installation_id"] == second["id"])
        .unwrap();
    assert_eq!(observed["title"], "Different title");
    assert_eq!(observed["origin"], "provider_response");
    assert!(observed["tags"].is_null());
    let sources = after["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(
        sources
            .iter()
            .all(|source| source["ready"] == true && source["recording_id"] == recording)
    );
    assert!(sources.iter().any(|source| source["id"] == primary_source));
    assert_ne!(sources[0]["id"], sources[1]["id"]);
    for source in sources {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/sources/{}/audio",
                        source["id"].as_str().unwrap()
                    ))
                    .header("authorization", format!("Bearer {owner}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let expected = if source["id"] == primary_source {
            &primary_audio
        } else {
            &alternate_audio
        };
        assert_eq!(bytes.as_ref(), expected.as_slice());
    }
    for (provider, source) in [(&primary, "primary-audio"), (&alternate, "alternate-audio")] {
        let requests = provider.requests("v1/audio").await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].1["source_id"], source);
    }
}

pub(super) async fn connect(app: &Router, owner: &str, provider: &ProviderServer) -> Value {
    let (status, service) = call(
        app,
        "POST",
        "/api/v1/plugins/services",
        json!({
            "name": "Owned attachment provider", "base_url": provider.url
        }),
        Some(owner),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, installation) = call(app, "POST", "/api/v1/plugins", json!({
        "service_id": service["id"], "service_revision": service["revision"], "bearer_token": null
    }), Some(owner)).await;
    assert_eq!(status, StatusCode::CREATED);
    installation
}

async fn get(app: &Router, owner: &str, path: &str) -> Value {
    let (status, value) = call(app, "GET", path, Value::Null, Some(owner)).await;
    assert_eq!(status, StatusCode::OK, "{path}");
    value
}

async fn state(app: &Router, owner: &str, recording: &str, playlist: &str) -> Value {
    json!({
        "recordings": get(app, owner, "/api/v1/recordings").await,
        "metadata": get(app, owner, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources": get(app, owner, &format!("/api/v1/recordings/{recording}/sources")).await,
        "collection": {
            "playlists": get(app, owner, "/api/v1/playlists").await,
            "playlist": get(app, owner, &format!("/api/v1/playlists/{playlist}")).await,
            "activity": get(app, owner, "/api/v1/activity").await,
            "events": get(app, owner, "/api/v1/sync?cursor=0").await,
        }
    })
}

async fn provider_requests(primary: &ProviderServer, alternate: &ProviderServer) -> Vec<usize> {
    let mut counts = Vec::new();
    for provider in [primary, alternate] {
        for path in [
            "v1/search",
            "v1/recordings/detail",
            "v1/sources",
            "v1/audio",
        ] {
            counts.push(provider.requests(path).await.len());
        }
    }
    counts
}
