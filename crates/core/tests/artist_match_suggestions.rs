#[allow(dead_code)]
#[path = "fixtures/artist_match_library.rs"]
mod library;

use hito_core::{
    ApplyArtistIdentityToCredit, ArtistMatchSuggestions, DismissArtistMatchSuggestion, Error,
    RecordingArtistIdentityState,
};
use library::{CREDIT, Library, M1, M2};
use uuid::Uuid;

async fn suggestions(library: &Library, credit: &str) -> ArtistMatchSuggestions {
    library
        .hito
        .catalog
        .artist_match_suggestions(&library.actor, credit)
        .await
        .unwrap()
}

async fn group(library: &Library, mbid: &str) -> String {
    library
        .hito
        .catalog
        .artist_identity_groups(&library.actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|group| group.provider_artist_id == mbid)
        .unwrap()
        .id
}

async fn imported_credit(library: &Library) -> String {
    library
        .hito
        .catalog
        .artists(&library.actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|artist| artist.name == CREDIT && artist.library_id != library.library)
        .unwrap()
        .id
}

async fn selected(library: &Library, recording: &str) -> bool {
    matches!(
        library
            .hito
            .catalog
            .recording_artist_identity_choice(&library.actor, recording)
            .await
            .unwrap()
            .state,
        RecordingArtistIdentityState::Selected(_)
    )
}

#[tokio::test]
async fn equal_names_are_ranked_by_shared_albums_and_never_applied_by_themselves() {
    let library = Library::prepare().await;
    library.run().await;
    let (g1, g2) = (group(&library, M1).await, group(&library, M2).await);
    let credit = library.credit(CREDIT).await.unwrap().id;
    let before = library.snapshot().await;

    let found = suggestions(&library, &credit).await;
    assert_eq!(found.dismissed, 0);
    let listed: Vec<(&str, i64, i64, i64)> = found
        .items
        .iter()
        .map(|item| {
            (
                item.group.id.as_str(),
                item.shared_albums,
                item.same_length_tracks,
                item.undecided_tracks,
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![(g1.as_str(), 1, 1, 1), (g2.as_str(), 0, 0, 1)],
        "The artist sharing an album and a same-length track ranks first; the namesake is still offered, weakly"
    );
    assert!(
        !selected(&library, &library.plain).await,
        "Suggestions never change a choice"
    );
    assert_eq!(library.snapshot().await["recordings"], before["recordings"]);

    let imported = suggestions(&library, &imported_credit(&library).await).await;
    assert_eq!(imported.items.len(), 2);
    assert!(
        imported
            .items
            .iter()
            .all(|item| (item.shared_albums, item.same_length_tracks) == (0, 0)),
        "An imported credit with no shared album or track is offered by name only"
    );
    let unrelated = suggestions(&library, &library.credit("Duo").await.unwrap().id).await;
    assert!(
        unrelated.items.is_empty(),
        "Only equal credit names are offered"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn dismissals_are_remembered_per_credit_and_do_not_hide_other_credits() {
    let library = Library::prepare().await;
    library.run().await;
    let (g1, g2) = (group(&library, M1).await, group(&library, M2).await);
    let credit = library.credit(CREDIT).await.unwrap().id;
    let dismissed = library
        .hito
        .catalog
        .dismiss_artist_match_suggestion(
            &library.actor,
            &credit,
            DismissArtistMatchSuggestion {
                group_id: g2.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(dismissed.dismissed, 1);
    assert_eq!(
        dismissed
            .items
            .iter()
            .map(|item| item.group.id.clone())
            .collect::<Vec<_>>(),
        vec![g1.clone()]
    );
    let again = library
        .hito
        .catalog
        .dismiss_artist_match_suggestion(
            &library.actor,
            &credit,
            DismissArtistMatchSuggestion {
                group_id: g2.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(again.dismissed, 1, "Dismissing twice is the same dismissal");
    assert_eq!(suggestions(&library, &credit).await.dismissed, 1);
    let imported = suggestions(&library, &imported_credit(&library).await).await;
    assert_eq!((imported.items.len(), imported.dismissed), (2, 0));
    assert!(matches!(
        library
            .hito
            .catalog
            .dismiss_artist_match_suggestion(
                &library.actor,
                &credit,
                DismissArtistMatchSuggestion {
                    group_id: "missing".into()
                },
            )
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        library
            .hito
            .catalog
            .dismiss_artist_match_suggestion(
                &library.actor,
                "missing",
                DismissArtistMatchSuggestion { group_id: g1 },
            )
            .await,
        Err(Error::NotFound)
    ));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn accepting_a_suggestion_uses_the_reviewed_path_and_ends_the_suggestion() {
    let library = Library::prepare().await;
    library.run().await;
    let g1 = group(&library, M1).await;
    let credit = library.credit(CREDIT).await.unwrap().id;
    let receipt = library
        .hito
        .catalog
        .apply_artist_identity_to_credit(
            &library.actor,
            &credit,
            ApplyArtistIdentityToCredit {
                request_id: Uuid::new_v4(),
                group_id: g1.clone(),
                after: None,
                limit: None,
            },
        )
        .await
        .unwrap();
    assert_eq!((receipt.applied, receipt.skipped_different), (1, 1));
    assert!(selected(&library, &library.plain).await);
    let after = suggestions(&library, &credit).await;
    assert!(
        after.items.is_empty(),
        "Nothing undecided is left under the credit; the namesake keeps its own artist"
    );
    library.origin.shutdown().await;
}
