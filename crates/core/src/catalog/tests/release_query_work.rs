#[path = "release_query_fixture.rs"]
mod fixture;

use crate::test_support::work_meter::WorkMeter;
use crate::{
    Actor, Artist, Browse, CoreConfig, Error, HostDeviceId, Recording, Release, ReleaseDetail,
    Services, open,
};
use fixture::{Expected, add_copies, assert_fixture, assert_statistics, seed};
use futures_util::FutureExt;
use serde_json::Value;
use std::{future::Future, panic::AssertUnwindSafe};

const TRACKS: usize = 96;
const PAGE_SIZE: i64 = 5;
const SMALL_COPIES: i64 = 32;
const LARGE_COPIES: i64 = 4096;
const WORK_PER_ADDED_COPY: u64 = 400;
const FIXED_WORK: u64 = 20_000;

struct Reads {
    work: Vec<(&'static str, u64)>,
    output: Vec<(&'static str, Value)>,
}

#[tokio::test]
async fn mixed_provider_album_reads_keep_source_history_work_proportional_to_copies() {
    let server = fixture::ProviderServer::start().await;
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let config = || {
        CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("release-query-work").unwrap(),
        )
    };
    let hito = open(config()).await.unwrap();
    let (owner_token, foreign_token, expected) = seed(&hito, &server).await;
    add_copies(
        &hito.catalog.store.pool,
        &expected.current_source,
        1,
        SMALL_COPIES,
    )
    .await;
    assert_fixture(&hito.catalog.store.pool, SMALL_COPIES).await;
    hito.catalog.store.pool.close().await;
    drop(hito);

    let hito = open(config()).await.unwrap();
    assert_statistics(&hito.catalog.store.pool, "absent", SMALL_COPIES).await;
    let baseline = read_catalog(
        hito,
        &owner_token,
        &foreign_token,
        &expected,
        "small-absent",
        SMALL_COPIES,
    )
    .await;

    let hito = open(config()).await.unwrap();
    fixture::prepare_statistics(&hito.catalog.store.pool, "fresh").await;
    assert_statistics(&hito.catalog.store.pool, "inherited", SMALL_COPIES).await;
    add_copies(
        &hito.catalog.store.pool,
        &expected.current_source,
        SMALL_COPIES + 1,
        LARGE_COPIES,
    )
    .await;
    assert_fixture(&hito.catalog.store.pool, LARGE_COPIES).await;
    hito.catalog.store.pool.close().await;
    drop(hito);

    let mut measurements = Vec::new();
    for statistics in ["inherited", "absent", "fresh"] {
        let hito = open(config()).await.unwrap();
        fixture::prepare_statistics(&hito.catalog.store.pool, statistics).await;
        hito.catalog.store.pool.close().await;
        drop(hito);

        let hito = open(config()).await.unwrap();
        let analyzed_copies = if statistics == "inherited" {
            SMALL_COPIES
        } else {
            LARGE_COPIES
        };
        assert_statistics(&hito.catalog.store.pool, statistics, analyzed_copies).await;
        assert_fixture(&hito.catalog.store.pool, LARGE_COPIES).await;
        let reads = read_catalog(
            hito,
            &owner_token,
            &foreign_token,
            &expected,
            statistics,
            LARGE_COPIES,
        )
        .await;
        measurements.push((statistics, reads));
    }
    for (statistics, reads) in measurements {
        assert_eq!(
            reads.output, baseline.output,
            "source history changed public output with {statistics} statistics"
        );
        assert_eq!(reads.work.len(), baseline.work.len());
        for ((operation, work), (baseline_operation, baseline_work)) in
            reads.work.iter().zip(&baseline.work)
        {
            assert_eq!(operation, baseline_operation);
            let budget = baseline_work
                + FIXED_WORK
                + (LARGE_COPIES - SMALL_COPIES) as u64 * WORK_PER_ADDED_COPY;
            assert!(
                *work > 0 && *work < budget,
                "Public catalog {operation} used approximately {work} VM steps with {statistics} statistics and {LARGE_COPIES} copies for {TRACKS} fixed tracks; small_history_work={baseline_work}, budget={budget}"
            );
        }
    }
    assert_eq!(server.requests("v1/recordings/detail").await.len(), 1);
    assert_eq!(server.requests("v1/sources").await.len(), 1);
    assert!(server.requests("v1/audio").await.is_empty());
}

async fn read_catalog(
    hito: Services,
    owner_token: &str,
    foreign_token: &str,
    expected: &Expected,
    statistics: &str,
    copies: i64,
) -> Reads {
    let owner = hito.identity.authenticate(owner_token).await.unwrap();
    let foreign = hito.identity.authenticate(foreign_token).await.unwrap();
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let exercise = AssertUnwindSafe(async {
        let mut work = Vec::new();
        let mut output = Vec::new();
        let (albums, steps) = measured(
            &meter,
            "albums",
            statistics,
            copies,
            hito.catalog.releases(
                &owner,
                Browse {
                    limit: Some(PAGE_SIZE),
                    ..Default::default()
                },
            ),
        )
        .await;
        assert_eq!(albums.total, 1);
        assert_eq!(albums.items.len(), 1);
        assert_release(&albums.items[0], expected);
        work.push(("albums", steps));
        output.push(("albums", serde_json::to_value(albums).unwrap()));
        for (operation, offset) in [
            ("album-first", 0),
            ("album-last", TRACKS as i64 - PAGE_SIZE),
        ] {
            let (detail, steps) = measured(
                &meter,
                operation,
                statistics,
                copies,
                hito.catalog.release(
                    &owner,
                    &expected.release,
                    Browse {
                        offset: Some(offset),
                        limit: Some(PAGE_SIZE),
                        ..Default::default()
                    },
                ),
            )
            .await;
            assert_detail(&detail, expected, offset as usize);
            work.push((operation, steps));
            output.push((operation, serde_json::to_value(detail).unwrap()));
        }
        let (artists, steps) = measured(
            &meter,
            "artists",
            statistics,
            copies,
            hito.catalog.artists(
                &owner,
                Browse {
                    limit: Some(PAGE_SIZE),
                    ..Default::default()
                },
            ),
        )
        .await;
        assert_eq!(artists.total, 1);
        assert_eq!(artists.items.len(), 1);
        assert_artist(&artists.items[0], expected);
        work.push(("artists", steps));
        output.push(("artists", serde_json::to_value(artists).unwrap()));
        let (artist, steps) = measured(
            &meter,
            "artist",
            statistics,
            copies,
            hito.catalog.artist(
                &owner,
                &expected.artist,
                Browse {
                    limit: Some(PAGE_SIZE),
                    ..Default::default()
                },
            ),
        )
        .await;
        assert_artist(&artist.artist, expected);
        assert_eq!(artist.releases.total, 1);
        assert_eq!(artist.releases.items.len(), 1);
        assert_release(&artist.releases.items[0], expected);
        assert_eq!(artist.recordings.total, TRACKS as i64);
        assert_eq!(artist.recordings.items.len(), PAGE_SIZE as usize);
        for (index, recording) in artist.recordings.items.iter().enumerate() {
            assert_recording(recording, expected, index);
        }
        work.push(("artist", steps));
        output.push(("artist", serde_json::to_value(artist).unwrap()));
        let (filtered, steps) = measured(
            &meter,
            "artist-albums",
            statistics,
            copies,
            hito.catalog.releases(
                &owner,
                Browse {
                    artist: Some(expected.artist.clone()),
                    limit: Some(PAGE_SIZE),
                    ..Default::default()
                },
            ),
        )
        .await;
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items.len(), 1);
        assert_release(&filtered.items[0], expected);
        work.push(("artist-albums", steps));
        output.push(("artist-albums", serde_json::to_value(filtered).unwrap()));
        assert_private(&hito, &foreign, expected).await;
        let current = hito
            .catalog
            .sources(&owner, &expected.recordings[0])
            .await
            .unwrap();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].id, expected.current_source);
        assert!(current[0].ready);
        assert!(
            hito.catalog
                .sources(&owner, &expected.recordings[1])
                .await
                .unwrap()
                .is_empty()
        );
        Reads { work, output }
    })
    .catch_unwind()
    .await;
    meter.remove(&hito.catalog.store.pool).await;
    hito.catalog.store.pool.close().await;
    match exercise {
        Ok(reads) => reads,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

async fn measured<T>(
    meter: &WorkMeter,
    operation: &'static str,
    statistics: &str,
    copies: i64,
    read: impl Future<Output = crate::Result<T>>,
) -> (T, u64) {
    let (result, work) = meter.measure(read).await;
    eprintln!(
        "Mixed provider album public VM_steps~ operation={operation} statistics={statistics} current_tracks={TRACKS} source_copies={copies} page_size={PAGE_SIZE} work={work} succeeded={}",
        result.is_ok()
    );
    (
        result.expect("owned catalog read must succeed before its emergency work limit"),
        work,
    )
}

fn assert_release(release: &Release, expected: &Expected) {
    assert_eq!(release.id, expected.release);
    assert_eq!(release.title, "Owned work album");
    assert!(release.artist.is_none() && release.artist_id.is_none());
    assert_eq!(release.track_count, TRACKS as i64);
    assert_eq!(release.duration_ms, TRACKS as i64 * 1000);
    assert_eq!(
        release.artwork_recording_id.as_ref(),
        expected.recordings.iter().min()
    );
}

fn assert_detail(detail: &ReleaseDetail, expected: &Expected, offset: usize) {
    assert_release(&detail.release, expected);
    assert_eq!(detail.tracks.total, TRACKS as i64);
    assert_eq!(detail.tracks.offset, offset as i64);
    assert_eq!(detail.tracks.limit, PAGE_SIZE);
    assert_eq!(detail.tracks.items.len(), PAGE_SIZE as usize);
    for (position, track) in detail.tracks.items.iter().enumerate() {
        let index = offset + position;
        assert_recording(&track.recording, expected, index);
        assert_eq!(track.entry_id, expected.entries[index]);
        assert!(track.disc_number.is_none() && track.track_number.is_none());
    }
}

fn assert_recording(recording: &Recording, expected: &Expected, index: usize) {
    assert_eq!(recording.id, expected.recordings[index]);
    assert_eq!(
        recording.release_id.as_deref(),
        Some(expected.release.as_str())
    );
    assert_eq!(recording.title, format!("Owned work track {index:03}"));
    assert_eq!(recording.inherited_title, recording.title);
    assert_eq!(recording.artist, "Owned work performer");
    assert_eq!(recording.inherited_artist, recording.artist);
    assert_eq!(recording.album.as_deref(), Some("Owned work album"));
    assert_eq!(recording.duration_ms, 1000);
    assert_eq!(recording.available, index == 0);
    assert!(!recording.liked);
}

fn assert_artist(artist: &Artist, expected: &Expected) {
    assert_eq!(artist.id, expected.artist);
    assert_eq!(artist.name, "Owned work performer");
    assert_eq!(artist.release_count, 1);
    assert_eq!(artist.recording_count, TRACKS as i64);
    assert_eq!(
        artist.artwork_recording_id.as_deref(),
        Some(expected.recordings[0].as_str())
    );
}

async fn assert_private(hito: &Services, foreign: &Actor, expected: &Expected) {
    let albums = hito
        .catalog
        .releases(foreign, Browse::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 0);
    assert!(albums.items.is_empty());
    let artists = hito
        .catalog
        .artists(foreign, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 0);
    assert!(artists.items.is_empty());
    assert!(matches!(
        hito.catalog
            .release(foreign, &expected.release, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        hito.catalog
            .artist(foreign, &expected.artist, Browse::default())
            .await,
        Err(Error::NotFound)
    ));
}
