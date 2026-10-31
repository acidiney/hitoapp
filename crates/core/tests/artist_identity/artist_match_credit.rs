#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    ArtistAssociationEvidence, CreditEvidence, CreditEvidenceKind, ResetRecordingArtistIdentity,
};
use library::{CREDIT, Evidence, NAMESAKE, NOVA, NameLibrary};
use uuid::Uuid;

const RECORDING: &str = "0f700000-0000-4000-8000-0000000000aa";

fn code(index: usize) -> String {
    format!("GBAAA20{index:05}")
}

fn stating(index: usize) -> Evidence {
    Evidence {
        isrc: Some(code(index)),
        artist_urls: Vec::new(),
    }
}

/// Twenty-one tracks of one credit; the ones listed in `stated` carry an ISRC.
async fn credit(name: &str, stated: &[usize]) -> NameLibrary {
    let entries: Vec<(&str, String, u64, Evidence)> = (0..21)
        .map(|index| {
            let evidence = if stated.contains(&index) {
                stating(index)
            } else {
                Evidence::default()
            };
            (
                name,
                format!("Song {index}"),
                100_000 + index as u64,
                evidence,
            )
        })
        .collect();
    let entries: Vec<(&str, &str, u64, Evidence)> = entries
        .iter()
        .map(|(credit, title, length, evidence)| {
            (*credit, title.as_str(), *length, evidence.clone())
        })
        .collect();
    NameLibrary::prepare_with_evidence(&entries).await
}

fn names(library: &NameLibrary, index: usize, artists: &[&str]) {
    library.musicbrainz.isrc(
        &code(index),
        Reply::Recordings(vec![(
            RECORDING.into(),
            artists.iter().map(|id| (*id).to_owned()).collect(),
        )]),
    );
}

#[tokio::test]
async fn one_isrc_matched_track_identifies_the_credit_for_all_its_undecided_siblings() {
    let library = credit(CREDIT, &[0]).await;
    names(&library, 0, &[NOVA]);
    let receipt = library.run().await;
    assert_eq!(receipt.matched, 21);
    let group = library.group_of(&library.recordings[0]).await.unwrap();
    assert_eq!(
        library.associations(&library.recordings[0]).await[0].evidence,
        ArtistAssociationEvidence::MatchedIsrc
    );
    for sibling in &library.recordings[1..] {
        assert_eq!(library.group_of(sibling).await.as_ref(), Some(&group));
        let linked = library.associations(sibling).await;
        assert_eq!(linked[0].provider_artist_id, NOVA);
        assert_eq!(
            linked[0].evidence,
            ArtistAssociationEvidence::MatchedCreditEvidence
        );
        assert_eq!(
            linked[0].credit_evidence,
            Some(CreditEvidence {
                kind: CreditEvidenceKind::Isrc,
                tracks: 1
            })
        );
    }
    assert_eq!(library.rows("artist_association_credit_matches").await, 20);
    let credit = library
        .scalar(&format!(
            "SELECT id FROM account_artists WHERE name='{CREDIT}'"
        ))
        .await;
    let read = library
        .hito
        .artist_catalog
        .artist_match_suggestions(&library.actor, &credit)
        .await
        .unwrap();
    assert_eq!(read.undecided_tracks, 0);
    assert_eq!(read.chosen.len(), 1);
    assert_eq!(read.chosen[0].group.id, group);
    assert_eq!(
        (read.chosen[0].tracks, read.chosen[0].automatic_tracks),
        (21, 21),
        "The credit page can say the credit was identified automatically"
    );
    assert_eq!(
        library.musicbrainz.requests().len(),
        1,
        "Only the one ISRC is asked: the siblings follow without a lookup"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn siblings_with_evidence_of_their_own_are_decided_by_it_not_by_the_credit() {
    let library = credit(CREDIT, &[0, 1, 2, 3]).await;
    names(&library, 0, &[NOVA]);
    names(&library, 1, &[NOVA]);
    names(&library, 2, &[NAMESAKE]);
    names(&library, 3, &[NOVA, NAMESAKE]);
    let receipt = library.run().await;
    assert_eq!(receipt.matched, 2 + 1 + 17);
    let provider = |index: usize| {
        let library = &library;
        async move {
            library
                .associations(&library.recordings[index])
                .await
                .first()
                .map(|association| association.provider_artist_id.clone())
        }
    };
    assert_eq!(provider(2).await.as_deref(), Some(NAMESAKE));
    assert_eq!(
        provider(3).await,
        None,
        "A track whose own evidence names two artists stays undecided"
    );
    for index in [0, 1, 4, 20] {
        assert_eq!(provider(index).await.as_deref(), Some(NOVA));
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_credit_without_a_clear_majority_artist_follows_nothing() {
    let library = credit(CREDIT, &[0, 1]).await;
    names(&library, 0, &[NOVA]);
    names(&library, 1, &[NAMESAKE]);
    assert_eq!(library.run().await.matched, 2);
    assert!(library.group_of(&library.recordings[5]).await.is_none());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn personal_decisions_are_never_followed_over() {
    let library = credit(CREDIT, &[0]).await;
    names(&library, 0, &[NOVA]);
    let decided = &library.recordings[5];
    let choice = library
        .hito
        .artist_catalog
        .recording_artist_identity_choice(&library.actor, decided)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .reset_recording_artist_identity(
            &library.actor,
            decided,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(library.run().await.matched, 20);
    assert!(library.group_of(decided).await.is_none());
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_credit_that_lists_several_artists_names_none_of_them_for_its_siblings() {
    let library = credit("Nova Quartet & Guest", &[0]).await;
    names(&library, 0, &[NOVA]);
    assert_eq!(library.run().await.matched, 1);
    assert!(library.group_of(&library.recordings[1]).await.is_none());
    library.origin.shutdown().await;
}
