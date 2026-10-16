use super::{
    playlist_fixture,
    provider_fixture::ProviderServer,
    support::{admin, app, call},
};
use axum::{Router, http::StatusCode};
use hito_core::{Action, ImportProviderPlaylist, Operation};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn client_sync_rejects_server_publications_and_keeps_device_names_non_authoritative() {
    let (_directory, app) = app().await;
    let token = admin(&app).await;
    let provider = playlist_fixture::connect(&app, &token).await;
    let (status, receipt) = call(
        &app,
        "POST",
        &provider.import_path,
        serde_json::to_value(ImportProviderPlaylist {
            request_id: Uuid::new_v4(),
            installation_revision: provider.revision,
            playlist_reference: "https://provider.invalid/publication-input-selection".into(),
        })
        .unwrap(),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let imported = receipt["playlist_id"].as_str().unwrap();
    let detail_path = format!("/api/v1/playlists/{imported}");
    let before = public_state(&app, &token, &detail_path).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["playlists"].as_array().unwrap().len(), 1);
    assert_eq!(before["detail"]["entries"].as_array().unwrap().len(), 4);
    assert_eq!(before["feed"]["operations"].as_array().unwrap().len(), 1);
    let server_event = before["feed"]["operations"][0].clone();
    assert_eq!(
        server_event
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["publication", "sequence"]
    );
    let publication = server_event["publication"].clone();
    assert_eq!(publication["kind"], "playlist_imported");
    assert_eq!(publication["snapshot"]["playlist_id"], imported);
    let operation_id = Uuid::parse_str(publication["publication_id"].as_str().unwrap()).unwrap();
    let client_playlist = Uuid::new_v4();
    let operation = serde_json::to_value(Operation {
        id: operation_id,
        action: Action::PlaylistCreate {
            playlist_id: client_playlist,
            name: "Client copy".into(),
        },
    })
    .unwrap();
    let valid = json!({"device_id": "server", "operations": [operation]});
    let mut action_injection = valid.clone();
    action_injection["operations"][0]["action"] = publication.clone();
    let mut attempts = vec![
        ("publication as Action".to_owned(), action_injection),
        (
            "read event as Operation".to_owned(),
            json!({"device_id": "server", "operations": [server_event]}),
        ),
    ];
    for pointer in ["", "/operations/0", "/operations/0/action"] {
        for (field, value) in [
            ("origin", json!("playlist_import")),
            ("publication", publication.clone()),
        ] {
            let mut body = valid.clone();
            body.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(field.into(), value);
            attempts.push((format!("extra field {pointer}/{field}"), body));
        }
    }
    let observed = provider_requests(&provider.server).await;
    assert_eq!(observed, [2, 1, 0, 0, 0, 0]);
    assert_eq!(attempts.len(), 8);
    for (attempt, body) in attempts {
        let (status, _) = call(&app, "POST", "/api/v1/sync", body, Some(&token)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{attempt}");
        assert_eq!(
            public_state(&app, &token, &detail_path).await,
            before,
            "{attempt}"
        );
        assert_eq!(
            provider_requests(&provider.server).await,
            observed,
            "{attempt}"
        );
    }

    let (status, accepted) = call(&app, "POST", "/api/v1/sync", valid.clone(), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accepted, json!({"acknowledged": [operation_id]}));
    let after = public_state(&app, &token, &detail_path).await;
    assert_eq!(after["catalog"], before["catalog"]);
    assert_eq!(after["detail"], before["detail"]);
    assert_eq!(after["playlists"].as_array().unwrap().len(), 2);
    assert!(
        after["playlists"]
            .as_array()
            .unwrap()
            .iter()
            .any(|playlist| playlist["id"] == imported)
    );
    assert!(
        after["playlists"]
            .as_array()
            .unwrap()
            .iter()
            .any(|playlist| playlist["id"] == client_playlist.to_string()
                && playlist["name"] == "Client copy"
                && playlist["kind"] == "local")
    );
    let events = after["feed"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], server_event);
    let client_event = &events[1];
    assert!(
        client_event["sequence"].as_i64().unwrap() > server_event["sequence"].as_i64().unwrap()
    );
    assert_eq!(
        *client_event,
        json!({
            "sequence": client_event["sequence"], "device_id": "server", "operation": operation
        })
    );
    assert_eq!(after["feed"]["cursor"], client_event["sequence"]);
    assert_eq!(after["feed"]["has_more"], false);
    let (status, client_detail) = call(
        &app,
        "GET",
        &format!("/api/v1/playlists/{client_playlist}"),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(client_detail["playlist"]["name"], "Client copy");
    assert_eq!(client_detail["entries"], json!([]));
    assert!(client_detail["origin"].is_null());
    let (status, replay) = call(&app, "POST", "/api/v1/sync", valid, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, accepted);
    assert_eq!(public_state(&app, &token, &detail_path).await, after);
    assert_eq!(provider_requests(&provider.server).await, observed);
}

async fn public_state(app: &Router, token: &str, detail_path: &str) -> Value {
    let mut state = serde_json::Map::new();
    for (key, path) in [
        ("catalog", "/api/v1/recordings"),
        ("playlists", "/api/v1/playlists"),
        ("detail", detail_path),
        ("feed", "/api/v1/sync?cursor=0"),
    ] {
        let (status, body) = call(app, "GET", path, Value::Null, Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{key}");
        state.insert(key.into(), body);
    }
    Value::Object(state)
}

async fn provider_requests(server: &ProviderServer) -> [usize; 6] {
    [
        server.requests("v1/playlists/detail").await.len(),
        server.requests("v1/playlists/items").await.len(),
        server.requests("v1/recordings/detail").await.len(),
        server.requests("v1/sources").await.len(),
        server.requests("v1/search").await.len(),
        server.requests("v1/audio").await.len(),
    ]
}
