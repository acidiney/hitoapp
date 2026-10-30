#[path = "../fixtures/provider_artwork.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{Actor, ArtworkFetchStatus, ImportProviderPlaylist, Services};
use image::{ExtendedColorType, codecs::jpeg::JpegEncoder};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn jpeg_without_its_eoi_marker_is_not_retained_after_a_valid_jpeg_is_served_exactly() {
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode(&[200, 60, 30].repeat(4), 2, 2, ExtendedColorType::Rgb8)
        .unwrap();
    assert_eq!(&jpeg[jpeg.len() - 2..], &[0xff, 0xd9]);
    for missing_eoi in [false, true] {
        let bytes = if missing_eoi {
            jpeg[..jpeg.len() - 2].to_vec()
        } else {
            jpeg.clone()
        };
        let server = fixture::ArtworkServer::start_with(bytes, "image/jpeg").await;
        let (_directory, hito, owner, _shared_library, _music) =
            support::library("artwork-jpeg").await;
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
            .expect("the owned provider must start its JPEG body");
        server.release_image();
        assert!(
            tokio::time::timeout(Duration::from_secs(3), worker)
                .await
                .expect("JPEG validation must finish")
                .unwrap()
                .unwrap()
        );
        let finished = hito.catalog.artwork_fetch(&owner, &job.id).await.unwrap();
        if missing_eoi {
            assert!(
                matches!(finished.status, ArtworkFetchStatus::Failed),
                "JPEG bytes without the EOI marker must fail, got {:?}",
                finished.status
            );
            assert_eq!(finished.error.as_deref(), Some("invalid_artwork"));
            assert_eq!(finished.attempts, 1);
            assert!(
                hito.catalog
                    .artwork(&owner, &recording)
                    .await
                    .unwrap()
                    .is_none()
            );
        } else {
            assert!(matches!(finished.status, ArtworkFetchStatus::Ready));
            let artwork = hito
                .catalog
                .artwork(&owner, &recording)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(artwork.content_type, "image/jpeg");
            assert_eq!(artwork.bytes, jpeg);
        }
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
