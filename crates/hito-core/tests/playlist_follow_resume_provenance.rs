use hito_core::test_support::playlist_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus, PluginRefresh,
    RefreshFollowedPlaylist, RefreshPlugin,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn recording(occurrence: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": "recording/a", "title": "Owned provenance fixture recording",
        "artist": "Owned provenance fixture artist", "duration_ms": 123000
    }}})
}

#[tokio::test]
async fn unchanged_content_verified_under_resumed_approval_publishes_provenance_once_without_identity_churn()
 {
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) =
        support::library("follow-resume-provenance").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/resume-provenance",
        "unchanged-snapshot",
        vec![
            recording("first-a"),
            json!({"occurrence_id": "unknown", "item": {"kind": "unknown"}}),
            json!({"occurrence_id": "withdrawn", "item": {
                "kind": "unavailable", "provider_recording_id": "recording/withdrawn",
                "title": "Owned withdrawn fixture recording", "artist": "Owned fixture artist"
            }}),
            recording("duplicate-a"),
        ],
    )
    .await;
    let followed = hito
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/owned-resume-provenance".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let original = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(original.entries.len(), 4);
    assert_ne!(original.entries[0].entry_id, original.entries[3].entry_id);
    assert!(original.entries[0].recording_id.is_some());
    assert_eq!(
        original.entries[0].recording_id,
        original.entries[3].recording_id
    );
    assert!(original.entries[1].recording_id.is_none());
    assert!(original.entries[2].recording_id.is_none());
    assert!(!original.entries[1].available);
    assert!(!original.entries[2].available);
    let original_json = serde_json::to_value(&original).unwrap();
    let initial_status = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(initial_status.generation, 1);
    assert_eq!(initial_status.content_revision, 1);
    assert_eq!(original.follow_content_revision, Some(1));
    let initial_feed =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let initial_publication = initial_feed["operations"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["publication"]
        .clone();
    assert_eq!(initial_publication["kind"], "playlist_followed");
    assert_eq!(
        initial_publication["snapshot"]["recordings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(provider_requests(&server).await, [2, 1, 0, 0, 0, 0]);

    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": "Reviewed provenance fixture",
                "protocol_version": 1, "capabilities": ["playlists", "search"]
            }),
        )
        .await;
    let PluginRefresh::ReviewRequired { digest, .. } = hito
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("The changed fixture manifest must require explicit review");
    };
    let PluginRefresh::Updated {
        installation: approved,
    } = hito
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: Some(digest),
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("The exact reviewed fixture manifest must be accepted");
    };
    assert_eq!(approved.id, installation.id);
    assert_eq!(approved.service_id, installation.service_id);
    assert!(approved.revision > installation.revision);
    let settings = hito.plugins.settings(&actor).await.unwrap();
    let service = settings
        .services
        .iter()
        .find(|service| service.id == approved.service_id)
        .unwrap();
    let paused = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(paused.status, PlaylistFollowStatus::Paused);
    assert_eq!(paused.content_revision, initial_status.content_revision);
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        original_json
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        initial_feed
    );
    support::synchronize(
        &hito,
        &actor,
        vec![Action::PlaylistFollowResume {
            playlist_id: followed.playlist_id,
            expected_generation: paused.generation,
            installation_revision: approved.revision,
            service_revision: service.revision,
        }],
    )
    .await;
    let resumed = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(resumed.generation, 2);
    assert_eq!(resumed.status, PlaylistFollowStatus::Ready);
    assert!(resumed.error.is_none());
    assert_eq!(resumed.content_revision, initial_status.content_revision);
    assert_eq!(resumed.snapshot, initial_status.snapshot);
    assert_eq!(resumed.last_refreshed_at, initial_status.last_refreshed_at);
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        original_json
    );
    let resumed_feed =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let resumed_events = resumed_feed["operations"].as_array().unwrap();
    let initial_events = initial_feed["operations"].as_array().unwrap();
    assert_eq!(resumed_events.len(), initial_events.len() + 1);
    assert_eq!(
        &resumed_events[..initial_events.len()],
        initial_events.as_slice()
    );
    assert_eq!(provider_requests(&server).await, [2, 1, 0, 0, 0, 0]);

    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let verified = hito.collection.playlist(&actor, &playlist).await.unwrap();
    let origin = verified.origin.as_ref().unwrap();
    assert_eq!(
        origin.installation_revision, approved.revision,
        "A successful complete read under resumed approval must publish the newly verified provenance"
    );
    assert_eq!(origin.service_revision, service.revision);
    let verified_status = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(verified_status.generation, resumed.generation);
    assert_eq!(verified_status.status, PlaylistFollowStatus::Ready);
    assert!(verified_status.error.is_none());
    assert_eq!(
        verified_status.content_revision,
        initial_status.content_revision + 1
    );
    assert_eq!(verified_status.snapshot, initial_status.snapshot);
    assert_eq!(
        verified_status.next_refresh_at - verified_status.last_refreshed_at,
        3600
    );
    let mut expected_detail = original_json.clone();
    expected_detail["origin"]["installation_revision"] = json!(approved.revision);
    expected_detail["origin"]["service_revision"] = json!(service.revision);
    expected_detail["follow_content_revision"] = json!(verified_status.content_revision);
    assert_eq!(
        serde_json::to_value(&verified).unwrap(),
        expected_detail,
        "A provenance-only publication must retain every entry, including unknown identities"
    );
    let verified_feed =
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap();
    let verified_events = verified_feed["operations"].as_array().unwrap();
    assert_eq!(verified_events.len(), resumed_events.len() + 1);
    assert_eq!(
        &verified_events[..resumed_events.len()],
        resumed_events.as_slice()
    );
    let publication = &verified_events.last().unwrap()["publication"];
    assert_eq!(publication["kind"], "playlist_follow_refreshed");
    assert_eq!(publication["interval"], initial_publication["interval"]);
    assert_ne!(
        publication["publication_id"],
        initial_publication["publication_id"]
    );
    let mut expected_snapshot = initial_publication["snapshot"].clone();
    expected_snapshot["origin"]["installation_revision"] = json!(approved.revision);
    expected_snapshot["origin"]["service_revision"] = json!(service.revision);
    assert_eq!(publication["snapshot"], expected_snapshot);
    assert_eq!(provider_requests(&server).await, [4, 2, 0, 0, 0, 0]);

    hito.collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: Uuid::new_v4(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        expected_detail
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        verified_feed
    );
    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let unchanged = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(unchanged.generation, verified_status.generation);
    assert_eq!(unchanged.content_revision, verified_status.content_revision);
    assert_eq!(unchanged.snapshot, verified_status.snapshot);
    assert_eq!(unchanged.status, PlaylistFollowStatus::Ready);
    assert!(unchanged.error.is_none());
    assert_eq!(
        unchanged.next_refresh_at - unchanged.last_refreshed_at,
        3600
    );
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        expected_detail
    );
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor, 0).await.unwrap()).unwrap(),
        verified_feed
    );
    assert_eq!(provider_requests(&server).await, [6, 3, 0, 0, 0, 0]);
    let details = server.requests("v1/playlists/detail").await;
    assert!(details[1..].iter().all(|(_, request)| request
        == &json!({
            "kind": "canonical", "playlist_id": "playlist/resume-provenance"
        })));
    assert!(
        !hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(provider_requests(&server).await, [6, 3, 0, 0, 0, 0]);
}

async fn provider_requests(server: &fixture::ProviderServer) -> [usize; 6] {
    [
        server.requests("v1/playlists/detail").await.len(),
        server.requests("v1/playlists/items").await.len(),
        server.requests("v1/search").await.len(),
        server.requests("v1/recordings/detail").await.len(),
        server.requests("v1/sources").await.len(),
        server.requests("v1/audio").await.len(),
    ]
}
