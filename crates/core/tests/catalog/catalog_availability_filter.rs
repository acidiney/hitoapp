use hito_core::test_support::{playlist_service, provider_audio::AudioLibrary, support};
use hito_core::{Actor, ImportProviderPlaylist, RecordingAvailability, Search, Services};
use serde_json::json;
use std::collections::BTreeSet;
use uuid::Uuid;

type Combination = (bool, bool);

async fn page(
    hito: &Services,
    actor: &Actor,
    availability: Option<RecordingAvailability>,
    offset: i64,
    limit: i64,
) -> hito_core::Page<hito_core::Recording> {
    hito.catalog
        .recordings(
            actor,
            Search {
                availability,
                offset: Some(offset),
                limit: Some(limit),
                ..Search::default()
            },
        )
        .await
        .unwrap()
}

async fn paged_ids(
    hito: &Services,
    actor: &Actor,
    availability: Option<RecordingAvailability>,
) -> (i64, Vec<String>) {
    let first = page(hito, actor, availability, 0, 1).await;
    let mut ids: Vec<String> = first.items.iter().map(|item| item.id.clone()).collect();
    for offset in 1..first.total + 1 {
        let next = page(hito, actor, availability, offset, 1).await;
        assert_eq!(next.total, first.total, "Totals stay stable across pages");
        ids.extend(next.items.into_iter().map(|item| item.id));
    }
    (first.total, ids)
}

async fn assert_filters_partition(hito: &Services, actor: &Actor) -> BTreeSet<Combination> {
    let all = page(hito, actor, None, 0, 200).await;
    let ordered: Vec<String> = all.items.iter().map(|item| item.id.clone()).collect();
    let combinations = all
        .items
        .iter()
        .map(|item| (item.available, item.plays_from_services))
        .collect();
    let expected = |wanted: bool| -> Vec<String> {
        all.items
            .iter()
            .filter(|item| (item.available || item.plays_from_services) == wanted)
            .map(|item| item.id.clone())
            .collect()
    };
    for (availability, wanted) in [
        (RecordingAvailability::AvailableOrService, true),
        (RecordingAvailability::NoSource, false),
    ] {
        let (total, ids) = paged_ids(hito, actor, Some(availability)).await;
        assert_eq!(
            ids,
            expected(wanted),
            "{availability:?} keeps the default order"
        );
        assert_eq!(
            total,
            ids.len() as i64,
            "{availability:?} total counts the filtered rows"
        );
    }
    let (total, ids) = paged_ids(hito, actor, None).await;
    assert_eq!(ids, ordered);
    assert_eq!(total, all.total);
    combinations
}

#[tokio::test]
async fn availability_filters_partition_recordings_by_source_and_service_state() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let library = f
        .hito
        .catalog
        .libraries(&f.actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    support::scan(&f.hito, &f.actor, &library).await;

    let playlists = playlist_service::ProviderServer::start().await;
    let installed =
        playlist_service::connect(&f.hito, &f.actor, &playlists, &["playlists", "sources"]).await;
    playlist_service::playlist(
        &playlists,
        "playlist/filter",
        "v1",
        vec![
            json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
            "id": "bound", "title": "Bound", "artist": "Band", "duration_ms": 100000,
            "release": {"id": "album-bound", "title": "Album"}, "disc_number": 1, "track_number": 1,
            "revision": "v1"}}}),
        ],
    )
    .await;
    f.hito
        .collection
        .import_provider_playlist(
            &f.actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/filter".into(),
            },
        )
        .await
        .unwrap();

    let mut seen = assert_filters_partition(&f.hito, &f.actor).await;
    f.hito
        .plugins
        .remove(&f.actor, &installed.id, installed.revision)
        .await
        .unwrap();
    seen.extend(assert_filters_partition(&f.hito, &f.actor).await);
    assert_eq!(
        seen,
        BTreeSet::from([(false, false), (false, true), (true, false), (true, true)]),
        "The fixture covers every combination of the two row flags"
    );
}

#[tokio::test]
async fn availability_filters_combine_with_search_and_likes() {
    let (_directory, hito, actor, library, music) = support::library("availability-search").await;
    support::write_wav(&music.join("local.wav"), 3);
    support::scan(&hito, &actor, &library).await;
    let liked = Search {
        liked: Some(true),
        availability: Some(RecordingAvailability::AvailableOrService),
        ..Search::default()
    };
    assert_eq!(
        hito.catalog.recordings(&actor, liked).await.unwrap().total,
        0
    );
    let searched = |availability| Search {
        q: Some("local".into()),
        availability: Some(availability),
        ..Search::default()
    };
    assert_eq!(
        hito.catalog
            .recordings(&actor, searched(RecordingAvailability::AvailableOrService))
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, searched(RecordingAvailability::NoSource))
            .await
            .unwrap()
            .total,
        0
    );
}
