use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{Action, ArtistIdentityGroupQuery, Browse, ChooseRecordingArtistIdentity, Error};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

const LOCAL: &str = "ÉVORA Artist";
const IMPORTED: &str = "PrivateSecondaryLabel";

#[tokio::test]
async fn group_search_uses_catalog_unicode_prefix_policy_for_every_current_member_credit() {
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
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingArtistSet {
                recording_id: Uuid::parse_str(&library.local).unwrap(),
                artist: LOCAL.into(),
            },
            Action::RecordingArtistSet {
                recording_id: Uuid::parse_str(&library.imported).unwrap(),
                artist: IMPORTED.into(),
            },
        ],
    )
    .await;
    let imported_library = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|recording| recording.id == library.imported)
        .unwrap()
        .library_id;
    let mut selections = Vec::new();
    for recording in [&library.local, &library.imported] {
        let context = hito
            .artist_catalog
            .begin_artist_review(actor, recording, library.metadata_provider.clone())
            .await
            .unwrap();
        let association = hito
            .artist_catalog
            .confirm_artist_review(actor, &context, catalog::ALPHA)
            .await
            .unwrap();
        selections.push(
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
    let group = &selections[0].selection.as_ref().unwrap().group_id;
    assert_eq!(&selections[1].selection.as_ref().unwrap().group_id, group);
    let before = library.snapshot().await;
    let requests = library.catalog.requests();
    let origin = library.origin_requests().await;
    for term in ["evora", "evor", "ÉVO", "evor art"] {
        let legacy = hito
            .catalog
            .artists(
                actor,
                Browse {
                    q: Some(term.into()),
                    library: Some(library.shared.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            legacy.total, 0,
            "A reviewed credit is represented by its group instead of the credit index"
        );
        let found = hito
            .artist_catalog
            .artist_identity_groups(actor, query(term, Some(&library.shared)))
            .await
            .unwrap();
        assert_eq!(
            found.total, 1,
            "Group search must use the catalog Unicode and prefix policy"
        );
        assert_eq!(found.items[0].id, *group);
        assert_eq!(found.items[0].name.value, LOCAL);
        assert_eq!(found.items[0].name.recording_id, library.local);
        let credit = hito
            .catalog
            .artist(actor, &found.items[0].name.artist_id, Default::default())
            .await
            .unwrap();
        assert_eq!(credit.artist.name, LOCAL);
        assert_eq!(
            found.items[0].name.association_id,
            selections[0].selection.as_ref().unwrap().association_id
        );
        assert_eq!(
            (found.items[0].library_count, found.items[0].recording_count),
            (1, 1)
        );
    }
    for (term, scope) in [(IMPORTED, &library.shared), ("evora", &imported_library)] {
        let found = hito
            .artist_catalog
            .artist_identity_groups(actor, query(term, Some(scope)))
            .await
            .unwrap();
        assert_eq!(found.total, 0);
        assert!(found.items.is_empty());
    }
    let visible = hito
        .artist_catalog
        .artist_identity_groups(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(visible.total, 1);
    assert_eq!(
        (
            visible.items[0].library_count,
            visible.items[0].recording_count
        ),
        (2, 2)
    );
    let hidden_term = if visible.items[0].name.recording_id == library.local {
        IMPORTED
    } else {
        "evora"
    };
    let other_member = hito
        .artist_catalog
        .artist_identity_groups(actor, query(hidden_term, None))
        .await
        .unwrap();
    assert_eq!(
        other_member.total, 1,
        "A credit hidden from the credit index stays findable through any current member label"
    );
    assert_eq!(other_member.items[0].id, *group);
    assert_eq!(
        other_member.items[0].name.value,
        visible.items[0].name.value
    );
    let peer = hito
        .artist_catalog
        .artist_identity_groups(&library.peer, query("evora", None))
        .await
        .unwrap();
    assert_eq!(peer.total, 0);
    assert!(peer.items.is_empty());
    assert!(matches!(
        hito.artist_catalog
            .artist_identity_groups(actor, query(&"a".repeat(513), None))
            .await,
        Err(Error::Invalid(_))
    ));
    hito.identity
        .set_library_access(actor, &library.shared, &actor.user().id, false)
        .await
        .unwrap();
    let hidden = hito
        .artist_catalog
        .artist_identity_groups(actor, query("evora", None))
        .await
        .unwrap();
    assert_eq!(
        hidden.total, 0,
        "Retained credits in a revoked library must not enter group search"
    );
    assert!(hidden.items.is_empty());
    let remaining = hito
        .artist_catalog
        .artist_identity_groups(actor, query("private", None))
        .await
        .unwrap();
    assert_eq!(remaining.total, 1);
    assert_eq!(remaining.items[0].id, *group);
    assert_eq!(remaining.items[0].name.value, IMPORTED);
    assert_eq!(remaining.items[0].name.recording_id, library.imported);
    hito.identity
        .set_library_access(actor, &library.shared, &actor.user().id, true)
        .await
        .unwrap();
    assert_eq!(library.snapshot().await, before);
    assert_eq!(library.catalog.requests(), requests);
    assert_eq!(library.origin_requests().await, origin);
}

fn query(term: &str, library: Option<&str>) -> ArtistIdentityGroupQuery {
    ArtistIdentityGroupQuery {
        q: Some(term.into()),
        library: library.map(str::to_owned),
        ..Default::default()
    }
}
