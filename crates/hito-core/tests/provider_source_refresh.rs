#[path = "fixtures/source_refresh.rs"]
mod fixture;
mod support;

use fixture::Fixture;
use serde_json::json;

#[tokio::test]
async fn playback_refresh_updates_readiness_without_readopting_or_changing_metadata() {
    let f =
        Fixture::start(json!({"items":[{"id":"saved/audio", "label":"Original", "ready":false}]}))
            .await;
    let before = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(before.len(), 1);
    assert!(!before[0].ready);
    support::synchronize(
        &f.hito,
        &f.actor,
        vec![
            hito_core::Action::RecordingTitleSet {
                recording_id: uuid::Uuid::parse_str(&f.recording.id).unwrap(),
                title: "Personal title".into(),
            },
            hito_core::Action::LikeSet {
                recording_id: uuid::Uuid::parse_str(&f.recording.id).unwrap(),
                liked: true,
            },
        ],
    )
    .await;
    f.server
        .reply(
            "v1/recordings/detail",
            json!({"invalid":"must not be requested"}),
        )
        .await;
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"saved/audio", "label":"Ready now", "ready":true}]}),
        )
        .await;
    let refreshed = f
        .hito
        .catalog
        .refresh_sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].id, before[0].id);
    assert!(refreshed[0].ready);
    let metadata = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &f.recording.id, Default::default())
        .await
        .unwrap();
    assert_eq!(metadata.title.value, "Personal title");
    assert_eq!(metadata.title.inherited, "Accepted title");
    assert_eq!(metadata.artist.value, "Accepted artist");
    assert_eq!(metadata.observations.items.len(), 1);
    let recordings = f
        .hito
        .catalog
        .recordings(&f.actor, Default::default())
        .await
        .unwrap();
    assert!(recordings.items[0].available);
    assert!(recordings.items[0].liked);
    assert_eq!(recordings.items[0].release_id, f.recording.release_id);
    assert_eq!(f.server.requests("v1/recordings/detail").await.len(), 1);
    let calls = f.server.requests("v1/sources").await;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].1, json!({"recording_id":"saved/item"}));
    assert_eq!(calls[1].0["authorization"], "Bearer refresh-private-token");
    assert!(f.server.requests("v1/audio").await.is_empty());
}

#[tokio::test]
async fn newly_discovered_audio_keeps_the_accepted_album_appearance_and_observation() {
    let f = Fixture::start(json!({"items":[]})).await;
    let accepted_releases = f
        .hito
        .catalog
        .releases(&f.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(accepted_releases.total, 1);
    let accepted_release = &accepted_releases.items[0];
    assert_eq!(Some(&accepted_release.id), f.recording.release_id.as_ref());
    assert_eq!(accepted_release.title, "Accepted album");
    assert_eq!(accepted_release.artist.as_deref(), Some("Accepted artist"));
    assert_eq!(accepted_release.track_count, 1);
    assert_eq!(accepted_release.duration_ms, 1000);
    let accepted_album = f
        .hito
        .catalog
        .release(&f.actor, &accepted_release.id, Default::default())
        .await
        .unwrap();
    assert_eq!(accepted_album.tracks.total, 1);
    assert_eq!(accepted_album.tracks.items.len(), 1);
    let accepted_track = &accepted_album.tracks.items[0];
    assert_eq!(accepted_track.recording.id, f.recording.id);
    assert_eq!(accepted_track.recording.title, "Accepted title");
    assert_eq!(accepted_track.recording.artist, "Accepted artist");
    assert_eq!(accepted_track.track_number, Some(1));
    assert!(!accepted_track.recording.available);
    assert!(uuid::Uuid::parse_str(&accepted_track.entry_id).is_ok());
    assert!(
        f.hito
            .catalog
            .sources(&f.actor, &f.recording.id)
            .await
            .unwrap()
            .is_empty()
    );
    let before = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &f.recording.id, Default::default())
        .await
        .unwrap();
    f.server
        .reply(
            "v1/recordings/detail",
            json!({"invalid":"must not be requested"}),
        )
        .await;
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"new/audio", "label":"Now ready", "ready":true}]}),
        )
        .await;
    let sources = f
        .hito
        .catalog
        .refresh_sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].ready);
    assert_eq!(sources[0].recording_id, f.recording.id);
    assert_eq!(sources[0].provider, "plugin");
    let releases = f
        .hito
        .catalog
        .releases(&f.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(releases.total, 1);
    assert_eq!(Some(&releases.items[0].id), f.recording.release_id.as_ref());
    assert_eq!(releases.items[0].title, "Accepted album");
    assert_eq!(
        serde_json::to_value(&releases).unwrap(),
        serde_json::to_value(&accepted_releases).unwrap()
    );
    let album = f
        .hito
        .catalog
        .release(&f.actor, &releases.items[0].id, Default::default())
        .await
        .unwrap();
    assert_eq!(album.tracks.items[0].recording.id, f.recording.id);
    assert_eq!(album.tracks.items[0].track_number, Some(1));
    assert_eq!(album.tracks.total, 1);
    assert_eq!(album.tracks.items.len(), 1);
    assert_eq!(album.tracks.items[0].entry_id, sources[0].id);
    let mut expected_track = serde_json::to_value(accepted_track).unwrap();
    expected_track["entry_id"] = json!(sources[0].id);
    expected_track["available"] = json!(true);
    assert_eq!(
        serde_json::to_value(&album.tracks.items[0]).unwrap(),
        expected_track
    );
    let after = f
        .hito
        .catalog
        .recording_metadata(&f.actor, &f.recording.id, Default::default())
        .await
        .unwrap();
    assert_eq!(
        after.observations.items.len(),
        before.observations.items.len()
    );
    assert_eq!(
        after.selected_observation.as_ref().unwrap().id,
        before.selected_observation.as_ref().unwrap().id
    );
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    assert_eq!(f.server.requests("v1/recordings/detail").await.len(), 1);
    assert_eq!(f.server.requests("v1/sources").await.len(), 2);
    assert!(f.server.requests("v1/audio").await.is_empty());
}

#[tokio::test]
async fn delayed_refresh_cannot_replace_a_newer_accepted_source_snapshot() {
    let f =
        Fixture::start(json!({"items":[{"id":"initial/audio", "label":"Initial", "ready":true}]}))
            .await;
    let other = hito_core::open(hito_core::CoreConfig::new(
        f.directory.path().join("hito.db"),
        f.directory.path().join("music"),
        hito_core::HostDeviceId::new("source-refresh").unwrap(),
    ))
    .await
    .unwrap();
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"old/audio", "label":"Delayed", "ready":true}]}),
        )
        .await;
    let old_gate = f.server.hold("v1/sources").await;
    let old = {
        let catalog = f.hito.catalog.clone();
        let actor = f.actor.clone();
        let id = f.recording.id.clone();
        tokio::spawn(async move { catalog.refresh_sources(&actor, &id).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), old_gate.entered())
        .await
        .unwrap();
    f.server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"new/audio", "label":"Current", "ready":true}]}),
        )
        .await;
    let new_gate = f.server.hold("v1/sources").await;
    let new = {
        let catalog = other.catalog.clone();
        let actor = f.actor.clone();
        let id = f.recording.id.clone();
        tokio::spawn(async move { catalog.refresh_sources(&actor, &id).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), new_gate.entered())
        .await
        .unwrap();
    new_gate.release();
    let accepted = new.await.unwrap().unwrap();
    let current = accepted
        .iter()
        .find(|source| source.ready)
        .unwrap()
        .id
        .clone();
    old_gate.release();
    assert!(matches!(
        old.await.unwrap(),
        Err(hito_core::Error::Conflict(_))
    ));
    let after = f
        .hito
        .catalog
        .sources(&f.actor, &f.recording.id)
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(
        after
            .iter()
            .filter(|source| source.ready)
            .map(|source| source.id.as_str())
            .collect::<Vec<_>>(),
        [current.as_str()]
    );
}
