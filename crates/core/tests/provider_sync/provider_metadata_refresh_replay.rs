#[path = "../fixtures/provider_metadata_backfill.rs"]
mod fixture;
use hito_core::test_support::support;

use hito_core::{Action, Error, RefreshImportedPlaylistMetadata};
use uuid::Uuid;

#[tokio::test]
async fn a_completed_metadata_refresh_replays_its_receipt_after_the_copy_is_deleted() {
    let f = fixture::Library::start().await;
    let playlist = f.first.playlist_id.to_string();
    let detail = f
        .hito
        .collection
        .playlist(&f.actor, &playlist)
        .await
        .unwrap();
    let recording = detail.entries[0].recording_id.as_deref().unwrap();
    f.select_owned_alternate_source(recording).await;
    f.publish_verified_album().await;
    let request_id = Uuid::new_v4();
    let request = || RefreshImportedPlaylistMetadata {
        request_id,
        installation_revision: f.installation.revision,
    };
    let receipt = {
        let completed = f
            .hito
            .collection
            .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
            .await
            .unwrap();
        assert_eq!(completed.request_id, request_id);
        assert_eq!(completed.playlist_id, f.first.playlist_id);
        assert_eq!(completed.updated_recordings, 1);
        serde_json::to_vec(&completed).unwrap()
    };

    support::synchronize(
        &f.hito,
        &f.actor,
        vec![Action::PlaylistDelete {
            playlist_id: f.first.playlist_id,
        }],
    )
    .await;
    assert!(matches!(
        f.hito.collection.playlist(&f.actor, &playlist).await,
        Err(Error::NotFound)
    ));
    f.origin.offline().await;
    f.alternate.offline().await;
    let requests_before = provider_requests(&f).await;
    let events_before =
        serde_json::to_vec(&f.hito.collection.events(&f.actor, 0).await.unwrap()).unwrap();
    let metadata_before = serde_json::to_vec(
        &f.hito
            .catalog
            .recording_metadata(&f.actor, recording, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();
    let choice_before = serde_json::to_vec(
        &f.hito
            .catalog
            .source_choice(&f.actor, recording)
            .await
            .unwrap(),
    )
    .unwrap();

    let replayed = f
        .hito
        .collection
        .refresh_imported_playlist_metadata(&f.actor, &playlist, request())
        .await
        .expect("a lost response must remain replayable after the owner deletes the copy");
    assert!(
        serde_json::to_vec(&replayed).unwrap() == receipt,
        "An accepted metadata request must return its byte-identical immutable receipt"
    );
    assert!(
        matches!(
            f.hito
                .collection
                .refresh_imported_playlist_metadata(
                    &f.actor,
                    &playlist,
                    RefreshImportedPlaylistMetadata {
                        request_id,
                        installation_revision: f.installation.revision + 1,
                    }
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "A known request ID must still reject different request contents after deletion"
    );
    assert!(
        matches!(
            f.hito
                .collection
                .refresh_imported_playlist_metadata(
                    &f.actor,
                    &f.second.playlist_id.to_string(),
                    request()
                )
                .await,
            Err(Error::Conflict(_))
        ),
        "A known request ID must still reject a different owned playlist"
    );
    assert_eq!(
        provider_requests(&f).await,
        requests_before,
        "Replays and conflicting request IDs must not contact either provider"
    );
    assert!(
        serde_json::to_vec(&f.hito.collection.events(&f.actor, 0).await.unwrap()).unwrap()
            == events_before,
        "Receipt replay must not publish another collection event"
    );
    assert!(
        serde_json::to_vec(
            &f.hito
                .catalog
                .recording_metadata(&f.actor, recording, Default::default())
                .await
                .unwrap()
        )
        .unwrap()
            == metadata_before,
        "Receipt replay must preserve retained recording metadata"
    );
    assert!(
        serde_json::to_vec(
            &f.hito
                .catalog
                .source_choice(&f.actor, recording)
                .await
                .unwrap()
        )
        .unwrap()
            == choice_before,
        "Receipt replay must preserve the manual source choice"
    );
}

async fn provider_requests(f: &fixture::Library) -> Vec<(&'static str, usize, usize)> {
    let mut counts = Vec::new();
    for path in [
        "v1/manifest",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/search",
        "v1/audio",
    ] {
        counts.push((
            path,
            f.origin.requests(path).await.len(),
            f.alternate.requests(path).await.len(),
        ));
    }
    counts
}
