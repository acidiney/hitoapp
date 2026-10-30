use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    ArtistIdentityGroupBrowse, ArtistIdentityGroupQuery, ChooseRecordingArtistIdentity,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn group_library_recording_and_release_pages_have_independent_offsets_and_bounded_limits() {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn exercise(library: &library::Library) {
    let hito = &library.hito;
    let actor = &library.actor;
    let mut selected = Vec::new();
    for (recording, identity) in [
        (&library.local, catalog::ALPHA),
        (&library.imported, catalog::ALPHA),
        (&library.namesake, catalog::BETA),
    ] {
        let context = hito
            .artist_catalog
            .begin_artist_review(actor, recording, library.metadata_provider.clone())
            .await
            .unwrap();
        let association = hito
            .artist_catalog
            .confirm_artist_review(actor, &context, identity)
            .await
            .unwrap();
        selected.push(
            hito.artist_catalog
                .choose_recording_artist_identity(
                    actor,
                    recording,
                    ChooseRecordingArtistIdentity {
                        request_id: Uuid::new_v4(),
                        expected_revision: 0,
                        association_id: Uuid::parse_str(&association.id).unwrap(),
                        context,
                    },
                )
                .await
                .unwrap(),
        );
    }
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    let all = hito
        .artist_catalog
        .artist_identity_groups(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(all.total, 2);
    for index in 0..2 {
        let page = hito
            .artist_catalog
            .artist_identity_groups(
                actor,
                ArtistIdentityGroupQuery {
                    offset: Some(index),
                    limit: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            (page.total, page.offset, page.limit, page.items.len()),
            (2, index, 1, 1)
        );
        assert_eq!(page.items[0].id, all.items[index as usize].id);
    }
    let first = hito
        .artist_catalog
        .artist_identity_groups(
            actor,
            ArtistIdentityGroupQuery {
                offset: Some(-10),
                limit: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((first.offset, first.limit, first.items.len()), (0, 1, 1));
    assert_eq!(first.items[0].id, all.items[0].id);
    let large = hito
        .artist_catalog
        .artist_identity_groups(
            actor,
            ArtistIdentityGroupQuery {
                limit: Some(i64::MAX),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((large.limit, large.total), (200, 2));
    let group = &selected[0].selection.as_ref().unwrap().group_id;
    let full = hito
        .artist_catalog
        .artist_identity_group(actor, group, Default::default())
        .await
        .unwrap();
    assert_eq!(
        (
            full.libraries.total,
            full.recordings.total,
            full.releases.total
        ),
        (2, 2, 2)
    );
    for (library_offset, recording_offset, release_offset) in [(1, 0, 1), (0, 1, 0)] {
        let page = hito
            .artist_catalog
            .artist_identity_group(
                actor,
                group,
                ArtistIdentityGroupBrowse {
                    library_offset: Some(library_offset),
                    recording_offset: Some(recording_offset),
                    release_offset: Some(release_offset),
                    limit: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            (
                page.group.library_count,
                page.group.recording_count,
                page.group.release_count
            ),
            (2, 2, 2)
        );
        assert_eq!(
            (
                page.libraries.total,
                page.libraries.offset,
                page.libraries.limit,
                page.libraries.items.len()
            ),
            (2, library_offset, 1, 1)
        );
        assert_eq!(
            (
                page.recordings.total,
                page.recordings.offset,
                page.recordings.limit,
                page.recordings.items.len()
            ),
            (2, recording_offset, 1, 1)
        );
        assert_eq!(
            (
                page.releases.total,
                page.releases.offset,
                page.releases.limit,
                page.releases.items.len()
            ),
            (2, release_offset, 1, 1)
        );
        assert_eq!(
            page.libraries.items[0].library_id,
            full.libraries.items[library_offset as usize].library_id
        );
        assert_eq!(
            page.recordings.items[0].recording.id,
            full.recordings.items[recording_offset as usize]
                .recording
                .id
        );
        assert_eq!(
            page.releases.items[0].id,
            full.releases.items[release_offset as usize].id
        );
    }
    let first = hito
        .artist_catalog
        .artist_identity_group(
            actor,
            group,
            ArtistIdentityGroupBrowse {
                library_offset: Some(-1),
                recording_offset: Some(-2),
                release_offset: Some(-3),
                limit: Some(0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (
            first.libraries.offset,
            first.recordings.offset,
            first.releases.offset
        ),
        (0, 0, 0)
    );
    assert_eq!(
        (
            first.libraries.limit,
            first.recordings.limit,
            first.releases.limit
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (
            first.libraries.items.len(),
            first.recordings.items.len(),
            first.releases.items.len()
        ),
        (1, 1, 1)
    );
    let empty = hito
        .artist_catalog
        .artist_identity_group(
            actor,
            group,
            ArtistIdentityGroupBrowse {
                library_offset: Some(i64::MAX),
                recording_offset: Some(i64::MAX),
                release_offset: Some(i64::MAX),
                limit: Some(i64::MAX),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (
            empty.libraries.limit,
            empty.recordings.limit,
            empty.releases.limit
        ),
        (200, 200, 200)
    );
    assert_eq!(
        (
            empty.libraries.total,
            empty.recordings.total,
            empty.releases.total
        ),
        (2, 2, 2)
    );
    assert!(
        empty.libraries.items.is_empty()
            && empty.recordings.items.is_empty()
            && empty.releases.items.is_empty()
    );
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}
