#[allow(dead_code)]
#[path = "../fixtures/artist_isrc_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    ArtistAssociationEvidence, MatchArtistsFromProviderEvidence, MetadataProvider,
    ResetRecordingArtistIdentity, UpdateMetadataSettings,
};
use library::{ARTIST, IMPORTED_CODE, Library, OTHER_ARTIST, OUTAGE_CODE, UNKNOWN_CODE};
use uuid::Uuid;

async fn associations(library: &Library, recording: &str) -> Vec<hito_core::ArtistAssociation> {
    library
        .hito
        .artist_catalog
        .recording_artist_associations(&library.actor, recording)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_imported_isrc_that_names_one_artist_groups_with_the_tagged_local_track_without_a_click()
{
    let library = Library::prepare().await;
    library.tag_pass().await;
    let tagged_group = library.group_of(&library.tagged).await.expect("tag match");
    assert!(
        library.group_of(&library.imported).await.is_none(),
        "The imported track carries no artist identifier of its own"
    );
    let recordings = serde_json::to_value(
        library
            .hito
            .catalog
            .recordings(&library.actor, Default::default())
            .await
            .unwrap(),
    )
    .unwrap();

    let receipt = library.run().await;
    assert_eq!(
        (
            receipt.matched,
            receipt.uncertain,
            receipt.declined,
            receipt.unavailable
        ),
        (4, 6, 0, 2)
    );
    assert_eq!(
        library.group_of(&library.imported).await,
        Some(tagged_group.clone()),
        "One ISRC resolving to one MusicBrainz artist joins the group of the tagged local track"
    );
    let linked = associations(&library, &library.imported).await;
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].provider, MetadataProvider::BuiltinMusicBrainz);
    assert_eq!(linked[0].provider_artist_id, ARTIST);
    assert_eq!(linked[0].evidence, ArtistAssociationEvidence::MatchedIsrc);

    let local = library
        .group_of(&library.local_isrc)
        .await
        .expect("file tag ISRC");
    assert_ne!(local, tagged_group);
    assert_eq!(
        associations(&library, &library.local_isrc).await[0].provider_artist_id,
        OTHER_ARTIST
    );
    for uncertain in [
        &library.ambiguous,
        &library.two_artists,
        &library.unknown,
        &library.compilation,
        &library.outage,
        &library.without_isrc,
        &library.two_pages,
        &library.shared_page,
        &library.unlinked_page,
        &library.page_outage,
    ] {
        assert!(library.group_of(uncertain).await.is_none());
        assert!(associations(&library, uncertain).await.is_empty());
    }
    let groups = library
        .hito
        .artist_catalog
        .artist_identity_groups(&library.actor, Default::default())
        .await
        .unwrap();
    let counts: Vec<(String, i64)> = groups
        .items
        .iter()
        .map(|group| (group.provider_artist_id.clone(), group.recording_count))
        .collect();
    assert!(counts.contains(&(ARTIST.to_string(), 3)));
    assert!(counts.contains(&(OTHER_ARTIST.to_string(), 2)));
    assert_eq!(
        recordings,
        serde_json::to_value(
            library
                .hito
                .catalog
                .recordings(&library.actor, Default::default())
                .await
                .unwrap()
        )
        .unwrap(),
        "Matching never changes recordings, sources or playlist entries"
    );

    let lookups = library.musicbrainz.requests();
    let distinct: std::collections::BTreeSet<&String> = lookups.iter().collect();
    assert_eq!(distinct.len(), 12, "{lookups:?}");
    assert!(
        lookups.len() <= 13,
        "Only a code shared across batches may be asked twice: {lookups:?}"
    );
    assert!(lookups.contains(&format!(
        "/ws/2/isrc/{IMPORTED_CODE}?fmt=json&inc=artist-credits"
    )));
    assert_eq!(library.rows("artist_association_provider_matches").await, 4);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn an_outage_is_retried_later_while_settled_answers_are_not_asked_again() {
    let library = Library::prepare().await;
    assert_eq!(library.run().await.unavailable, 2);
    let before = library.musicbrainz.requests().len();
    let second = library.run().await;
    assert_eq!(
        (second.matched, second.uncertain, second.unavailable),
        (0, 0, 2)
    );
    let mut asked: Vec<String> = library.musicbrainz.requests()[before..].to_vec();
    asked.sort();
    assert_eq!(
        asked.len(),
        2,
        "Only the unavailable answers are asked again: {asked:?}"
    );
    assert!(asked.iter().any(|request| request.contains(OUTAGE_CODE)));
    library.musicbrainz.isrc(
        OUTAGE_CODE,
        Reply::Recordings(vec![(
            "0f700000-0000-4000-8000-0000000000a3".into(),
            vec![ARTIST.into()],
        )]),
    );
    library
        .musicbrainz
        .url(library::OUTAGE_PAGE, Reply::Artists(vec![ARTIST.into()]));
    let third = library.run().await;
    assert_eq!((third.matched, third.unavailable), (2, 0));
    assert!(library.group_of(&library.outage).await.is_some());
    assert!(library.group_of(&library.page_outage).await.is_some());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_new_provider_answer_waits_for_a_changed_observation() {
    let library = Library::prepare().await;
    library.musicbrainz.isrc(UNKNOWN_CODE, Reply::NotFound);
    library.run().await;
    assert!(library.group_of(&library.unknown).await.is_none());
    library.musicbrainz.isrc(
        UNKNOWN_CODE,
        Reply::Recordings(vec![(
            "0f700000-0000-4000-8000-0000000000a4".into(),
            vec![ARTIST.into()],
        )]),
    );
    let again = library.run().await;
    assert_eq!(again.matched, 0);
    assert!(
        library.group_of(&library.unknown).await.is_none(),
        "An unchanged observation is not examined again"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn personal_decisions_are_never_overridden_and_decided_recordings_are_not_looked_up() {
    let library = Library::prepare().await;
    let choice = library
        .hito
        .artist_catalog
        .recording_artist_identity_choice(&library.actor, &library.imported)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .reset_recording_artist_identity(
            &library.actor,
            &library.imported,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    let receipt = library.run().await;
    assert_eq!(
        receipt.matched, 3,
        "Every recording but the decided one is matched"
    );
    assert!(library.group_of(&library.imported).await.is_none());
    assert!(
        !library
            .musicbrainz
            .requests()
            .iter()
            .any(|request| request.contains(IMPORTED_CODE)),
        "A recording the account has decided about is not looked up"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn disabled_metadata_enrichment_looks_nothing_up() {
    let library = Library::prepare().await;
    let settings = library
        .hito
        .artist_catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
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
    let receipt = library.run().await;
    assert_eq!(
        (receipt.matched, receipt.uncertain, receipt.unavailable),
        (0, 0, 0)
    );
    assert!(library.musicbrainz.requests().is_empty());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_request_that_matched_replays_exactly_without_asking_the_provider_again() {
    let library = Library::prepare().await;
    let mut after = None;
    let (request, first) = loop {
        let request = MatchArtistsFromProviderEvidence {
            request_id: Uuid::new_v4(),
            after: after.clone(),
            limit: Some(3),
        };
        let receipt = library
            .hito
            .artist_catalog
            .match_artists_from_provider_evidence(&library.actor, request.clone())
            .await
            .unwrap();
        if receipt.matched > 0 {
            break (request, receipt);
        }
        after = Some(
            receipt
                .next_after
                .clone()
                .expect("a request matches something"),
        );
    };
    let requests = library.musicbrainz.requests().len();
    let replay = library
        .hito
        .artist_catalog
        .match_artists_from_provider_evidence(&library.actor, request.clone())
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(library.musicbrainz.requests().len(), requests);
    let changed = library
        .hito
        .artist_catalog
        .match_artists_from_provider_evidence(
            &library.actor,
            MatchArtistsFromProviderEvidence {
                limit: Some(1),
                ..request
            },
        )
        .await;
    assert!(matches!(changed, Err(hito_core::Error::Conflict(_))));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn batches_resume_from_a_cursor_until_every_recording_is_examined() {
    let library = Library::prepare().await;
    let mut matched = 0;
    let mut after = None;
    let mut batches = 0;
    loop {
        let receipt = library
            .hito
            .artist_catalog
            .match_artists_from_provider_evidence(
                &library.actor,
                MatchArtistsFromProviderEvidence {
                    request_id: Uuid::new_v4(),
                    after: after.clone(),
                    limit: Some(2),
                },
            )
            .await
            .unwrap();
        matched += receipt.matched;
        batches += 1;
        assert!(batches <= 8, "The walk must end");
        match receipt.next_after {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(matched, 4);
    assert!(batches >= 4);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_pass_cancelled_while_the_provider_is_slow_changes_nothing_and_can_be_repeated() {
    let library = Library::prepare().await;
    library
        .musicbrainz
        .delay_replies(std::time::Duration::from_secs(3));
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        library
            .hito
            .artist_catalog
            .match_artists_from_provider_evidence(
                &library.actor,
                MatchArtistsFromProviderEvidence {
                    request_id: Uuid::new_v4(),
                    after: None,
                    limit: None,
                },
            ),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "The pass is still waiting for the provider"
    );
    assert_eq!(library.rows("artist_association_provider_matches").await, 0);
    assert_eq!(library.rows("artist_provider_checks").await, 0);
    library.musicbrainz.delay_replies(std::time::Duration::ZERO);
    assert_eq!(library.run().await.matched, 4);
    library.origin.shutdown().await;
}
