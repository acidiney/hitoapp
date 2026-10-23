use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
use futures_util::FutureExt;
use hito_core::test_support::support;
use hito_core::{Artist, Browse, ChooseRecordingArtistIdentity, ResetRecordingArtistIdentity};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn reviewed_identities_replace_covered_library_credits_and_namesakes_stay_apart() {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn credits(library: &library::Library, actor: &hito_core::Actor) -> Vec<Artist> {
    library
        .hito
        .catalog
        .artists(
            actor,
            Browse {
                q: Some(catalog::CREDIT.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
}

async fn choose(library: &library::Library, recording: &str, identity: &str) -> String {
    let context = library
        .hito
        .catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = library
        .hito
        .catalog
        .confirm_artist_review(&library.actor, &context, identity)
        .await
        .unwrap();
    let choice = library
        .hito
        .catalog
        .recording_artist_identity_choice(&library.actor, recording)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .choose_recording_artist_identity(
            &library.actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context,
            },
        )
        .await
        .unwrap()
        .selection
        .unwrap()
        .group_id
}

async fn exercise(library: &library::Library) {
    let hito = &library.hito;
    let actor = &library.actor;
    let before = library.snapshot().await;

    let origin = library.origin_requests().await;
    let unreviewed = credits(library, actor).await;
    assert_eq!(
        unreviewed.len(),
        2,
        "Without reviewed evidence, equal names in two libraries stay separate credits"
    );
    assert_ne!(unreviewed[0].library_id, unreviewed[1].library_id);
    let all_before = hito
        .catalog
        .artists(actor, Default::default())
        .await
        .unwrap();

    let alpha = choose(library, &library.local, catalog::ALPHA).await;
    assert_eq!(
        choose(library, &library.imported, catalog::ALPHA).await,
        alpha
    );
    let partial = credits(library, actor).await;
    assert_eq!(
        partial.len(),
        1,
        "The local credit is fully covered by the reviewed identity and leaves the index"
    );
    assert_ne!(partial[0].library_id, library.shared);
    assert_eq!(
        partial[0].recording_count, 2,
        "The imported credit still holds an unreviewed namesake recording and stays visible"
    );

    let beta = choose(library, &library.namesake, catalog::BETA).await;
    assert_ne!(alpha, beta);
    assert!(
        credits(library, actor).await.is_empty(),
        "Fully reviewed credits are represented by their identity groups"
    );
    let all_after = hito
        .catalog
        .artists(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(all_after.total, all_before.total - 2);
    assert!(all_after.items.iter().all(|a| a.name != catalog::CREDIT));
    assert_eq!(
        hito.catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        2
    );
    for artist in &unreviewed {
        let page = hito
            .catalog
            .artist(actor, &artist.id, Default::default())
            .await
            .expect("Covered credit pages remain reachable by identifier");
        assert_eq!(page.artist.name, catalog::CREDIT);
    }
    assert_eq!(
        hito.catalog
            .artists(&library.peer, Default::default())
            .await
            .unwrap()
            .items
            .iter()
            .filter(|a| a.name == catalog::CREDIT)
            .count(),
        1,
        "Another account's credits are not affected by this account's reviews"
    );

    let choice = hito
        .catalog
        .recording_artist_identity_choice(actor, &library.local)
        .await
        .unwrap();
    hito.catalog
        .reset_recording_artist_identity(
            actor,
            &library.local,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    let reset = credits(library, actor).await;
    assert_eq!(
        reset.len(),
        1,
        "Reset returns the local credit to the index"
    );
    assert_eq!(reset[0].library_id, library.shared);

    assert_eq!(
        library.origin_requests().await,
        origin,
        "Index reads must not contact the provider origin"
    );
    assert_eq!(
        library.snapshot().await,
        before,
        "Recordings, sources, playlists, likes, progress and credit pages stay unchanged"
    );
}
