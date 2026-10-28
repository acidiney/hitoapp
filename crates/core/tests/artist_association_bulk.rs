use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[allow(dead_code)]
#[path = "fixtures/artist_identity_selection_library.rs"]
mod library;
use futures_util::{FutureExt, future::BoxFuture};
#[allow(unused_imports)]
use hito_core::test_support::support;
use hito_core::{
    Actor, ApplyArtistIdentityToCredit, Artist, ArtistIdentityCreditReceipt, Browse,
    ChooseRecordingArtistIdentity, Error, RecordingArtistIdentityChoice,
    RecordingArtistIdentityState, UnlinkRecordingArtist,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

async fn run(exercise: for<'a> fn(&'a library::Library) -> BoxFuture<'a, ()>) {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn credits(library: &library::Library) -> Vec<Artist> {
    library
        .hito
        .catalog
        .artists(
            &library.actor,
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
    let catalog = &library.hito.catalog;
    let context = catalog
        .begin_artist_review(&library.actor, recording, library.metadata_provider.clone())
        .await
        .unwrap();
    let association = catalog
        .confirm_artist_review(&library.actor, &context, identity)
        .await
        .unwrap();
    let choice = catalog
        .recording_artist_identity_choice(&library.actor, recording)
        .await
        .unwrap();
    catalog
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

async fn choice(
    library: &library::Library,
    actor: &Actor,
    recording: &str,
) -> RecordingArtistIdentityChoice {
    library
        .hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap()
}

fn group_of(choice: &RecordingArtistIdentityChoice) -> Option<&str> {
    match &choice.state {
        RecordingArtistIdentityState::Selected(selection) => Some(&selection.group_id),
        _ => None,
    }
}

async fn apply(
    library: &library::Library,
    actor: &Actor,
    credit: &str,
    request: ApplyArtistIdentityToCredit,
) -> Result<ArtistIdentityCreditReceipt, Error> {
    library
        .hito
        .catalog
        .apply_artist_identity_to_credit(actor, credit, request)
        .await
}

fn request(group: &str) -> ApplyArtistIdentityToCredit {
    ApplyArtistIdentityToCredit {
        request_id: Uuid::new_v4(),
        group_id: group.into(),
        after: None,
        limit: None,
    }
}

async fn imported_credit(library: &library::Library) -> String {
    credits(library)
        .await
        .into_iter()
        .find(|artist| artist.library_id != library.shared)
        .unwrap()
        .id
}

async fn association_of(library: &library::Library, recording: &str) -> String {
    library
        .hito
        .catalog
        .recording_artist_associations(&library.actor, recording)
        .await
        .unwrap()
        .remove(0)
        .id
}

async fn unlink(library: &library::Library, recording: &str, association: &str) {
    let catalog = &library.hito.catalog;
    let state = catalog
        .recording_artist_association_changes(&library.actor, recording)
        .await
        .unwrap();
    catalog
        .unlink_recording_artist(
            &library.actor,
            recording,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: state.revision,
                association_id: Uuid::parse_str(association).unwrap(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn bulk_apply_does_not_relink_an_association_the_account_unlinked() {
    run(|library| {
        Box::pin(async move {
            let alpha = choose(library, &library.local, catalog::ALPHA).await;
            let credit = imported_credit(library).await;
            let first = apply(library, &library.actor, &credit, request(&alpha))
                .await
                .unwrap();
            assert!(first.applied >= 1);
            let association = association_of(library, &library.imported).await;
            unlink(library, &library.imported, &association).await;
            let second = apply(library, &library.actor, &credit, request(&alpha))
                .await
                .unwrap();
            assert_eq!(second.applied, 0);
            assert_eq!(second.unchanged, first.applied - 1);
            assert_eq!(second.skipped_different, 1);
            assert!(
                library
                    .hito
                    .catalog
                    .recording_artist_associations(&library.actor, &library.imported)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(group_of(&choice(library, &library.actor, &library.imported).await).is_none());
        })
    })
    .await;
}
