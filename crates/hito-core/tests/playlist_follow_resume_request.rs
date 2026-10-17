#[path = "fixtures/playlist_service.rs"]
mod fixture;
mod support;

use hito_core::{
    Action, Actor, Error, FollowProviderPlaylist, PlaylistFollowInterval, PlaylistFollowStatus,
    PluginInstallation, PluginRefresh, RefreshFollowedPlaylist, RefreshPlugin, Services,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

fn recording(occurrence: &str, recording: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": format!("recording/{recording}"), "title": format!("Recording {recording}"),
        "artist": "Owned request fixture artist", "duration_ms": 123000
    }}})
}

#[tokio::test]
async fn a_new_manual_request_waits_for_an_obsolete_lease_then_retries_into_the_resumed_generation()
{
    let server = fixture::ProviderServer::start().await;
    let (_directory, hito, actor, _shared, _music) =
        support::library("follow-resume-request").await;
    let installation = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
    fixture::playlist(
        &server,
        "playlist/resume-request",
        "last-good",
        vec![recording("first-a", "a"), recording("duplicate-a", "a")],
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
                playlist_reference: "https://provider.invalid/owned-resume-request".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let original = hito.collection.playlist(&actor, &playlist).await.unwrap();
    let old_request = Uuid::new_v4();
    let old_receipt = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: old_request,
            },
        )
        .await
        .unwrap();
    let old_receipt_json = serde_json::to_value(&old_receipt).unwrap();
    let gate = server.hold("v1/playlists/detail").await;
    let worker = hito
        .collection
        .run_next_follow_refresh_until(std::future::pending());
    tokio::pin!(worker);
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            biased;
            _ = gate.entered() => {},
            _ = &mut worker => panic!("The actual old-generation worker must enter its held provider request"),
        }
    }).await.expect("The owned provider must observe the running old generation");
    assert_eq!(
        hito.collection
            .playlist_follow(&actor, &playlist)
            .await
            .unwrap()
            .status,
        PlaylistFollowStatus::Running
    );
    let approved = approve_connection(&hito, &actor, &server, &installation).await;
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
    let before = public_state(&hito, &actor, &playlist).await;
    let observed = provider_requests(&server).await;
    assert_eq!(observed, [3, 1, 0, 0, 0, 0]);

    let replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: old_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), old_receipt_json);
    assert_eq!(public_state(&hito, &actor, &playlist).await, before);
    assert_eq!(provider_requests(&server).await, observed);
    let fresh_request = Uuid::new_v4();
    let fresh = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: fresh_request,
            },
        )
        .await;
    assert!(
        matches!(fresh, Err(Error::Conflict(_))),
        "A new manual request must not attach its receipt to an obsolete generation's running job"
    );
    assert_eq!(public_state(&hito, &actor, &playlist).await, before);
    assert_eq!(provider_requests(&server).await, observed);

    gate.open();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut worker)
            .await
            .expect("The obsolete worker must retire after provider release")
            .unwrap()
    );
    assert_eq!(public_state(&hito, &actor, &playlist).await, before);
    fixture::playlist(
        &server,
        "playlist/resume-request",
        "current-generation",
        vec![
            recording("duplicate-a", "a"),
            recording("added-b", "b"),
            recording("first-a", "a"),
        ],
    )
    .await;
    let requests_after_retirement = provider_requests(&server).await;
    let accepted = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: fresh_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(accepted.request_id, fresh_request);
    assert_eq!(accepted.playlist_id, followed.playlist_id);
    assert_ne!(accepted.job_id, old_receipt.job_id);
    let accepted_json = serde_json::to_value(&accepted).unwrap();
    let queued = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(queued.generation, 2);
    assert_eq!(queued.status, PlaylistFollowStatus::Queued);
    assert_eq!(
        serde_json::to_value(hito.collection.playlist(&actor, &playlist).await.unwrap()).unwrap(),
        before["detail"]
    );
    assert_eq!(provider_requests(&server).await, requests_after_retirement);
    let queued_state = public_state(&hito, &actor, &playlist).await;
    let replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: fresh_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), accepted_json);
    assert_eq!(public_state(&hito, &actor, &playlist).await, queued_state);
    assert_eq!(provider_requests(&server).await, requests_after_retirement);

    assert!(
        hito.collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    let current = hito
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(current.generation, 2);
    assert_eq!(current.status, PlaylistFollowStatus::Ready);
    assert_eq!(current.snapshot, "current-generation");
    assert!(current.error.is_none());
    let updated = hito.collection.playlist(&actor, &playlist).await.unwrap();
    assert_eq!(updated.playlist.id, original.playlist.id);
    assert_eq!(updated.entries.len(), 3);
    assert_eq!(updated.entries[0].entry_id, original.entries[1].entry_id);
    assert_eq!(updated.entries[2].entry_id, original.entries[0].entry_id);
    assert_eq!(
        updated.entries[0].recording_id,
        original.entries[0].recording_id
    );
    assert_eq!(
        updated.entries[2].recording_id,
        original.entries[0].recording_id
    );
    assert!(
        !original
            .entries
            .iter()
            .any(|entry| entry.entry_id == updated.entries[1].entry_id)
    );
    let completed = public_state(&hito, &actor, &playlist).await;
    let events = completed["feed"]["operations"].as_array().unwrap();
    let previous_events = before["feed"]["operations"].as_array().unwrap();
    assert_eq!(events.len(), previous_events.len() + 1);
    assert_eq!(&events[..previous_events.len()], previous_events.as_slice());
    assert_eq!(
        events.last().unwrap()["publication"]["kind"],
        "playlist_follow_refreshed"
    );
    assert_eq!(
        completed["catalog"]["total"].as_i64().unwrap(),
        before["catalog"]["total"].as_i64().unwrap() + 1
    );
    let after_publication = provider_requests(&server).await;
    let replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: fresh_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), accepted_json);
    let replay = hito
        .collection
        .enqueue_follow_refresh(
            &actor,
            &playlist,
            RefreshFollowedPlaylist {
                request_id: old_request,
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), old_receipt_json);
    assert!(
        !hito
            .collection
            .run_next_follow_refresh_until(std::future::pending())
            .await
            .unwrap()
    );
    assert_eq!(public_state(&hito, &actor, &playlist).await, completed);
    assert_eq!(provider_requests(&server).await, after_publication);
    assert_eq!(server.requests("v1/sources").await.len(), 0);
    assert_eq!(server.requests("v1/audio").await.len(), 0);
}

async fn approve_connection(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    installation: &PluginInstallation,
) -> PluginInstallation {
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.playlist-fixture", "name": "Reviewed request fixture",
                "protocol_version": 1, "capabilities": ["playlists", "search"]
            }),
        )
        .await;
    let PluginRefresh::ReviewRequired { digest, .. } = hito
        .plugins
        .refresh(
            actor,
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
            actor,
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
    approved
}

async fn public_state(hito: &Services, actor: &Actor, playlist: &str) -> Value {
    json!({
        "detail": hito.collection.playlist(actor, playlist).await.unwrap(),
        "status": hito.collection.playlist_follow(actor, playlist).await.unwrap(),
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "feed": hito.collection.events(actor, 0).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap()
    })
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
