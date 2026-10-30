#[path = "../fixtures/provider_artwork.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{Actor, ArtworkFetchStatus, ImportProviderPlaylist, Services};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn a_png_with_a_corrupt_iend_crc_is_rejected_without_publishing_artwork() {
    let mut bytes = fixture::COVER.to_vec();
    *bytes.last_mut().unwrap() ^= 1;
    assert_invalid_png_is_not_retained(bytes).await;
}

#[tokio::test]
async fn a_png_with_a_corrupt_ancillary_crc_after_idat_is_rejected_without_publishing_artwork() {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[255, 0, 0]).unwrap();
    writer
        .write_chunk(png::chunk::tEXt, b"fixture\0owned")
        .unwrap();
    writer.finish().unwrap();
    let ancillary_crc_end = bytes.len() - 12;
    bytes[ancillary_crc_end - 1] ^= 1;
    assert_invalid_png_is_not_retained(bytes).await;
}

async fn assert_invalid_png_is_not_retained(bytes: Vec<u8>) {
    let server = fixture::ArtworkServer::start_with(bytes, "image/png").await;
    let (_directory, hito, owner, _shared_library, _music) =
        support::library("artwork-validation").await;
    let installed = server.connect(&hito, &owner).await;
    let playlist = hito
        .collection
        .import_provider_playlist(
            &owner,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-selection".into(),
            },
        )
        .await
        .unwrap()
        .playlist_id;
    let recording = hito
        .collection
        .playlist(&owner, &playlist.to_string())
        .await
        .unwrap()
        .entries[0]
        .recording_id
        .clone()
        .unwrap();
    let before = saved_state(&hito, &owner, playlist, &recording).await;
    let requests = server.requests().await;
    let job = hito
        .catalog
        .enqueue_recording_artwork(&owner, &recording)
        .await
        .unwrap();
    let worker = {
        let catalog = hito.catalog.clone();
        tokio::spawn(async move {
            catalog
                .run_next_artwork_job_until(std::future::pending())
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), server.image_entered())
        .await
        .expect("the owned provider must start its image body");
    server.release_image();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .expect("invalid image validation must finish")
            .unwrap()
            .unwrap()
    );

    let failed = hito.catalog.artwork_fetch(&owner, &job.id).await.unwrap();
    assert!(
        matches!(failed.status, ArtworkFetchStatus::Failed),
        "a corrupt PNG must fail validation, got {:?}",
        failed.status
    );
    assert_eq!(failed.error.as_deref(), Some("invalid_artwork"));
    assert_eq!(failed.attempts, 1);
    assert!(
        hito.catalog
            .artwork(&owner, &recording)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        saved_state(&hito, &owner, playlist, &recording).await,
        before
    );
    let after = server.requests().await;
    assert_eq!(&after[..requests.len()], requests.as_slice());
    assert_eq!(
        &after[requests.len()..],
        &[json!({
            "method": "POST", "path": "v1/artwork",
            "body": {"entity": "recording", "provider_id": fixture::PROVIDER_RECORDING_ID}
        })]
    );
}

async fn saved_state(hito: &Services, owner: &Actor, playlist: Uuid, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(owner, Default::default()).await.unwrap(),
        "playlist": hito.collection.playlist(owner, &playlist.to_string()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(owner, recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(owner, recording).await.unwrap(),
        "activity": hito.collection.activity(owner).await.unwrap(),
        "events": hito.collection.events(owner, 0).await.unwrap()
    })
}
