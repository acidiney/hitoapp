#[path = "fixtures/artist_identity_selection_catalog.rs"]
mod catalog;
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
mod support;

use futures_util::FutureExt;
use hito_core::{
    ArtistCandidatePreviewAvailability, ArtistCandidateSearchAvailability,
    ArtistIdentityGroupBrowse, ArtistIdentityGroupQuery, ChooseRecordingArtistIdentity,
    RecordingArtistIdentityState,
};
use hito_plugin_sdk::ArtistSearchRequest;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn explicit_performer_choices_group_local_and_imported_tracks_without_absorbing_a_namesake() {
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
    let before = library.snapshot().await;
    let mut reviews = Vec::new();
    for (recording, identity) in [
        (&library.local, catalog::ALPHA),
        (&library.imported, catalog::ALPHA),
        (&library.namesake, catalog::BETA),
    ] {
        let context = hito
            .catalog
            .begin_artist_review(actor, recording, library.metadata_provider.clone())
            .await
            .unwrap();
        let found = hito
            .catalog
            .search_artist_candidates_until(
                actor,
                &context,
                ArtistSearchRequest {
                    query: catalog::CREDIT.into(),
                    cursor: None,
                    snapshot: None,
                    limit: 2,
                },
                std::future::pending(),
            )
            .await
            .unwrap();
        let ArtistCandidateSearchAvailability::Available { page, .. } = found else {
            panic!("Owned artist candidates must be available");
        };
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].artist.name, page.items[1].artist.name);
        assert_ne!(page.items[0].artist.id, page.items[1].artist.id);
        let preview = hito
            .catalog
            .preview_artist_candidate_until(actor, &context, identity, std::future::pending())
            .await
            .unwrap();
        let ArtistCandidatePreviewAvailability::Available { profile, .. } = preview else {
            panic!("The exact selected profile must be available");
        };
        assert_eq!(profile.artist.id, identity);
        let association = hito
            .catalog
            .confirm_artist_review(actor, &context, identity)
            .await
            .unwrap();
        reviews.push((recording, context, association));
    }
    assert_eq!(library.snapshot().await, before);
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        0,
        "Immutable review history must not implicitly select a group"
    );
    let catalog_before = library.catalog.requests();
    let origin_before = library.origin_requests().await;
    let mut receipts = Vec::new();
    for (recording, context, association) in reviews {
        let choice = hito
            .catalog
            .recording_artist_identity_choice(actor, recording)
            .await
            .unwrap();
        assert!(
            choice.revision == 0
                && matches!(choice.state, RecordingArtistIdentityState::Unselected)
        );
        let receipt = hito
            .catalog
            .choose_recording_artist_identity(
                actor,
                recording,
                ChooseRecordingArtistIdentity {
                    request_id: Uuid::new_v4(),
                    expected_revision: choice.revision,
                    association_id: Uuid::parse_str(&association.id).unwrap(),
                    context,
                },
            )
            .await
            .unwrap();
        assert_eq!(receipt.revision, 1);
        assert_eq!(
            receipt.selection.as_ref().unwrap().association_id,
            association.id
        );
        receipts.push(receipt);
    }
    let group_id = &receipts[0].selection.as_ref().unwrap().group_id;
    assert_eq!(&receipts[1].selection.as_ref().unwrap().group_id, group_id);
    assert_ne!(&receipts[2].selection.as_ref().unwrap().group_id, group_id);
    let groups = hito
        .catalog
        .artist_identity_groups(
            actor,
            ArtistIdentityGroupQuery {
                q: Some(catalog::CREDIT.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        groups.total, 2,
        "Equal labels must retain distinct selected provider identities"
    );
    assert_eq!(groups.items.len(), 2);
    assert!(
        groups
            .items
            .iter()
            .all(|group| group.name.value == catalog::CREDIT
                && group.provider == library.metadata_provider)
    );
    let detail = hito
        .catalog
        .artist_identity_group(actor, group_id, Default::default())
        .await
        .unwrap();
    assert_eq!(detail.group.provider_artist_id, catalog::ALPHA);
    assert_eq!(
        (
            detail.group.library_count,
            detail.group.recording_count,
            detail.group.release_count
        ),
        (2, 2, 2)
    );
    assert_eq!(detail.libraries.total, 2);
    assert_eq!(detail.recordings.total, 2);
    assert_eq!(detail.releases.total, 2);
    let mut actual = detail
        .recordings
        .items
        .iter()
        .map(|member| member.recording.id.clone())
        .collect::<Vec<_>>();
    let mut expected = vec![library.local.clone(), library.imported.clone()];
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    assert!(
        detail
            .recordings
            .items
            .iter()
            .any(
                |member| member.recording.id == detail.group.name.recording_id
                    && member.artist_id == detail.group.name.artist_id
                    && member.association_id == detail.group.name.association_id
            )
    );
    let mut expected_releases = hito
        .catalog
        .recordings_by_ids(
            actor,
            &[
                Uuid::parse_str(&library.local).unwrap(),
                Uuid::parse_str(&library.imported).unwrap(),
            ],
        )
        .await
        .unwrap()
        .into_iter()
        .map(|recording| recording.release_id.unwrap())
        .collect::<Vec<_>>();
    let mut actual_releases = detail
        .releases
        .items
        .iter()
        .map(|release| release.id.clone())
        .collect::<Vec<_>>();
    expected_releases.sort();
    actual_releases.sort();
    assert_eq!(actual_releases, expected_releases);
    let scoped = hito
        .catalog
        .artist_identity_group(
            actor,
            group_id,
            ArtistIdentityGroupBrowse {
                library: Some(library.shared.clone()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (
            scoped.group.library_count,
            scoped.group.recording_count,
            scoped.group.release_count
        ),
        (1, 1, 1)
    );
    assert_eq!(scoped.libraries.total, 1);
    assert_eq!(scoped.libraries.items[0].library_id, library.shared);
    assert_eq!(scoped.recordings.items[0].recording.id, library.local);
    assert_eq!(scoped.releases.items.len(), 1);
    let namesake = hito
        .catalog
        .artist_identity_group(
            actor,
            &receipts[2].selection.as_ref().unwrap().group_id,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(namesake.group.provider_artist_id, catalog::BETA);
    assert_eq!(namesake.recordings.total, 1);
    assert_eq!(namesake.recordings.items[0].recording.id, library.namesake);
    assert_eq!(namesake.releases.total, 0);
    assert_eq!(
        hito.catalog
            .artist_identity_groups(&library.peer, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        library.snapshot().await,
        before,
        "Grouping must preserve recordings, releases, legacy credits, sources, original audio, exact metadata/operations, duplicate entries, import receipts/origin, likes/progress/events and peer state"
    );
    assert_eq!(
        library.catalog.requests(),
        catalog_before,
        "Selection and group browsing must make no catalog calls"
    );
    assert_eq!(
        library.origin_requests().await,
        origin_before,
        "Grouping must not fetch provider metadata, sources or audio"
    );
}
