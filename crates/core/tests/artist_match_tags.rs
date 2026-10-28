#[allow(dead_code)]
#[path = "fixtures/artist_match_library.rs"]
mod library;

use hito_core::{
    ArtistAssociationEvidence, ArtistCreditRole, AssociateRecordingArtist, Error,
    MatchArtistsFromTags, MetadataProvider, RecordingArtistIdentityState,
    ResetRecordingArtistIdentity, UnlinkRecordingArtist, UpdateMetadataSettings,
};
use library::{CREDIT, Library, M1, M2, SPELLED};
use uuid::Uuid;

async fn group_of(library: &Library, recording: &str) -> Option<String> {
    match library
        .hito
        .catalog
        .recording_artist_identity_choice(&library.actor, recording)
        .await
        .unwrap()
        .state
    {
        RecordingArtistIdentityState::Selected(selection) => Some(selection.group_id),
        _ => None,
    }
}

async fn associations(library: &Library, recording: &str) -> Vec<hito_core::ArtistAssociation> {
    library
        .hito
        .catalog
        .recording_artist_associations(&library.actor, recording)
        .await
        .unwrap()
}

#[tokio::test]
async fn embedded_artist_ids_group_recordings_but_equal_names_and_ambiguous_tags_never_merge() {
    let library = Library::prepare().await;
    let before = library.snapshot().await;
    let receipt = library.run().await;
    assert_eq!(
        (
            receipt.matched,
            receipt.skipped_ambiguous,
            receipt.skipped_declined
        ),
        (3, 1, 0)
    );
    assert!(receipt.next_after.is_none());

    let a = group_of(&library, &library.a).await.expect("A is matched");
    let b = group_of(&library, &library.b).await.expect("B is matched");
    let namesake = group_of(&library, &library.namesake)
        .await
        .expect("N is matched");
    assert_eq!(
        a, b,
        "One embedded artist id groups two differently spelled credits"
    );
    assert_ne!(
        a, namesake,
        "Two artist ids sharing a name stay in separate groups"
    );
    for untouched in [&library.plain, &library.multi, &library.imported] {
        assert!(group_of(&library, untouched).await.is_none());
        assert!(associations(&library, untouched).await.is_empty());
    }

    let linked = associations(&library, &library.a).await;
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].provider, MetadataProvider::BuiltinMusicBrainz);
    assert_eq!(linked[0].provider_artist_id, M1);
    assert_eq!(linked[0].role, ArtistCreditRole::RecordingArtist);
    assert_eq!(
        linked[0].evidence,
        ArtistAssociationEvidence::MatchedFileTagArtistId
    );

    let groups = library
        .hito
        .catalog
        .artist_identity_groups(&library.actor, Default::default())
        .await
        .unwrap();
    assert_eq!(groups.total, 2);
    let counts: Vec<(String, i64)> = groups
        .items
        .iter()
        .map(|group| (group.provider_artist_id.clone(), group.recording_count))
        .collect();
    assert!(counts.contains(&(M1.to_string(), 2)));
    assert!(counts.contains(&(M2.to_string(), 1)));

    assert!(
        library.credit(SPELLED).await.is_none(),
        "A credit fully covered by a matched artist leaves the credits list"
    );
    assert!(
        library.credit(CREDIT).await.is_some(),
        "The namesake credit still holds an unmatched track and an imported track"
    );
    let after = library.snapshot().await;
    assert_eq!(
        before["recordings"], after["recordings"],
        "Matching never changes recordings, sources or playlist entries"
    );
    assert_eq!(before["detail"], after["detail"]);
    assert_eq!(before["activity"], after["activity"]);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_request_replays_exactly_and_a_second_pass_changes_nothing() {
    let library = Library::prepare().await;
    let request = MatchArtistsFromTags {
        request_id: Uuid::new_v4(),
        after: None,
        limit: None,
    };
    let first = library
        .hito
        .catalog
        .match_artists_from_tags(&library.actor, request.clone())
        .await
        .unwrap();
    let replay = library
        .hito
        .catalog
        .match_artists_from_tags(&library.actor, request.clone())
        .await
        .unwrap();
    assert_eq!(first, replay);
    let changed = library
        .hito
        .catalog
        .match_artists_from_tags(
            &library.actor,
            MatchArtistsFromTags {
                limit: Some(1),
                ..request
            },
        )
        .await;
    assert!(matches!(changed, Err(Error::Conflict(_))));
    let second = library.run().await;
    assert_eq!(
        second.matched, 0,
        "Matched recordings are not matched again"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn batches_resume_from_a_cursor_until_every_recording_is_checked() {
    let library = Library::prepare().await;
    let mut matched = 0;
    let mut after = None;
    let mut batches = 0;
    loop {
        let receipt = library
            .hito
            .catalog
            .match_artists_from_tags(
                &library.actor,
                MatchArtistsFromTags {
                    request_id: Uuid::new_v4(),
                    after: after.clone(),
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        matched += receipt.matched;
        batches += 1;
        assert!(batches <= 6, "The walk must end");
        match receipt.next_after {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(matched, 3);
    assert!(batches >= 4);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn personal_resets_and_unlinks_are_never_overridden() {
    let library = Library::prepare().await;
    library.run().await;
    let choice = library
        .hito
        .catalog
        .recording_artist_identity_choice(&library.actor, &library.b)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .reset_recording_artist_identity(
            &library.actor,
            &library.b,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    let linked = associations(&library, &library.namesake).await;
    let changes = library
        .hito
        .catalog
        .recording_artist_association_changes(&library.actor, &library.namesake)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .unlink_recording_artist(
            &library.actor,
            &library.namesake,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: changes.revision,
                association_id: Uuid::parse_str(&linked[0].id).unwrap(),
            },
        )
        .await
        .unwrap();
    let again = library.run().await;
    assert_eq!(again.matched, 0);
    assert!(group_of(&library, &library.b).await.is_none());
    assert!(group_of(&library, &library.namesake).await.is_none());
    assert!(associations(&library, &library.namesake).await.is_empty());
    assert!(group_of(&library, &library.a).await.is_some());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn an_earlier_unlink_without_a_choice_is_respected() {
    let library = Library::prepare().await;
    let settings = library
        .hito
        .catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    let credit = library.credit(CREDIT).await.unwrap();
    let metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &library.a, Default::default())
        .await
        .unwrap();
    let observation = metadata.selected_observation.unwrap();
    let association = library
        .hito
        .catalog
        .associate_recording_artist(
            &library.actor,
            &library.a,
            AssociateRecordingArtist {
                artist_id: credit.id.clone(),
                source_id: observation.source_id.clone(),
                observation_id: observation.id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: MetadataProvider::BuiltinMusicBrainz,
                provider_artist_id: M1.into(),
                policy_revision: settings.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        association.evidence,
        ArtistAssociationEvidence::UserConfirmed
    );
    let changes = library
        .hito
        .catalog
        .recording_artist_association_changes(&library.actor, &library.a)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .unlink_recording_artist(
            &library.actor,
            &library.a,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: changes.revision,
                association_id: Uuid::parse_str(&association.id).unwrap(),
            },
        )
        .await
        .unwrap();
    let receipt = library.run().await;
    assert_eq!((receipt.matched, receipt.skipped_declined), (2, 1));
    assert!(group_of(&library, &library.a).await.is_none());
    assert!(associations(&library, &library.a).await.is_empty());
    assert!(group_of(&library, &library.b).await.is_some());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn disabled_metadata_enrichment_matches_nothing() {
    let library = Library::prepare().await;
    let settings = library
        .hito
        .catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    let off = library
        .hito
        .catalog
        .set_metadata_settings(
            &library.actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: settings.providers.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(library.run().await.matched, 0);
    let _ = off;
    for id in library.all() {
        assert!(group_of(&library, id).await.is_none());
    }
    library.origin.shutdown().await;
}
