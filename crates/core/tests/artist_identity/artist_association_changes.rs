#[allow(dead_code)]
#[path = "../fixtures/artist_catalog_profiles_library.rs"]
mod library;
use hito_core::test_support::artist_catalog_profiles_service as provider;
use hito_core::test_support::metadata_tag_logs as logs;
#[allow(unused_imports)]
use hito_core::test_support::support;

use futures_util::{FutureExt, future::BoxFuture};
use hito_core::{
    ArtistAssociation, ArtistAssociationChangeReceipt, ArtistCreditRole, ArtistProfileAvailability,
    AssociateRecordingArtist, Browse, ChooseRecordingArtistIdentity, Error, MetadataHistoryQuery,
    MetadataProvider, RecordingArtistIdentityState, UndoRecordingArtistChange,
    UnlinkRecordingArtist,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

async fn with_provider<F>(exercise: F)
where
    F: for<'a> FnOnce(&'a library::Library, &'a provider::CatalogServer) -> BoxFuture<'a, ()>,
{
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(exercise(&library, &server))
        .catch_unwind()
        .await;
    let joined = server.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
    assert!(joined, "The owned HTTP fixture must join shutdown");
}

async fn unlink(
    library: &library::Library,
    request: Uuid,
    revision: i64,
    association: &str,
) -> Result<ArtistAssociationChangeReceipt, Error> {
    library
        .hito
        .catalog
        .unlink_recording_artist(
            &library.actor,
            &library.recording_id,
            UnlinkRecordingArtist {
                request_id: request,
                expected_revision: revision,
                association_id: Uuid::parse_str(association).unwrap(),
            },
        )
        .await
}

async fn undo(
    library: &library::Library,
    request: Uuid,
    revision: i64,
) -> Result<ArtistAssociationChangeReceipt, Error> {
    library
        .hito
        .catalog
        .undo_recording_artist_change(
            &library.actor,
            &library.recording_id,
            UndoRecordingArtistChange {
                request_id: request,
                expected_revision: revision,
            },
        )
        .await
}

async fn revision(library: &library::Library) -> i64 {
    library
        .hito
        .catalog
        .recording_artist_association_changes(&library.actor, &library.recording_id)
        .await
        .unwrap()
        .revision
}

async fn listed(library: &library::Library) -> Vec<String> {
    library
        .hito
        .catalog
        .recording_artist_associations(&library.actor, &library.recording_id)
        .await
        .unwrap()
        .into_iter()
        .map(|association| association.id)
        .collect()
}

async fn profile(
    library: &library::Library,
    association: &str,
) -> Result<ArtistProfileAvailability, Error> {
    library
        .hito
        .catalog
        .recording_artist_profile_until(
            &library.actor,
            &library.recording_id,
            association,
            std::future::pending(),
        )
        .await
}

async fn associate(
    library: &library::Library,
    provider: &MetadataProvider,
    artist: &str,
) -> Result<ArtistAssociation, Error> {
    let settings = library
        .hito
        .catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .catalog
        .associate_recording_artist(
            &library.actor,
            &library.recording_id,
            AssociateRecordingArtist {
                artist_id: library.artist_id.clone(),
                source_id: Some(library.source_id.clone()),
                observation_id: library.observation_id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: provider.clone(),
                provider_artist_id: artist.into(),
                policy_revision: settings.revision,
            },
        )
        .await
}

#[tokio::test]
async fn unlinking_denies_the_profile_and_undo_restores_the_same_evidence() {
    with_provider(|library, server| {
        async move {
            let url = server.url.clone();
            let association = library.review_artist(&url, provider::ARTIST_ID).await;
            assert_eq!(revision(library).await, 1);
            assert!(matches!(
                profile(library, &association.id).await.unwrap(),
                ArtistProfileAvailability::Available { .. }
            ));
            let before = library.snapshot().await;
            let requests = server.counts().await;

            let first = Uuid::new_v4();
            let unlinked = unlink(library, first, 1, &association.id).await.unwrap();
            assert_eq!(unlinked.revision, 2);
            assert!(!unlinked.linked);
            assert_eq!(unlinked.association_id, association.id);
            assert!(listed(library).await.is_empty());
            assert!(matches!(
                profile(library, &association.id).await,
                Err(Error::NotFound)
            ));
            assert_eq!(
                server.counts().await,
                requests,
                "a denied profile makes no provider request"
            );

            assert_eq!(
                unlink(library, first, 1, &association.id).await.unwrap(),
                unlinked,
                "an exact replay returns the accepted receipt"
            );
            assert_eq!(revision(library).await, 2);
            assert!(matches!(
                unlink(library, first, 1, &Uuid::new_v4().to_string()).await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                unlink(library, Uuid::new_v4(), 1, &association.id).await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                unlink(library, Uuid::new_v4(), 2, &association.id).await,
                Err(Error::NotFound)
            ));

            let offered = library
                .hito
                .catalog
                .recording_artist_association_changes(&library.actor, &library.recording_id)
                .await
                .unwrap();
            let offered = offered.undo.unwrap();
            assert_eq!(offered.association_id, association.id);
            assert!(offered.linked);

            let second = Uuid::new_v4();
            assert!(matches!(
                undo(library, second, 1).await,
                Err(Error::Conflict(_))
            ));
            let restored = undo(library, second, 2).await.unwrap();
            assert_eq!(restored.revision, 3);
            assert!(restored.linked);
            assert_eq!(undo(library, second, 2).await.unwrap(), restored);
            assert_eq!(revision(library).await, 3);
            assert_eq!(listed(library).await, vec![association.id.clone()]);
            assert!(matches!(
                profile(library, &association.id).await.unwrap(),
                ArtistProfileAvailability::Available { .. }
            ));

            let reverted = undo(library, Uuid::new_v4(), 3).await.unwrap();
            assert!(!reverted.linked);
            assert!(listed(library).await.is_empty());
            assert_eq!(library.snapshot().await, before);
        }
        .boxed()
    })
    .await;
}

#[tokio::test]
async fn undoing_a_new_link_removes_it_and_reviewing_again_reuses_its_evidence() {
    with_provider(|library, server| {
        async move {
            let url = server.url.clone();
            let association = library.review_artist(&url, provider::ARTIST_ID).await;
            let removed = undo(library, Uuid::new_v4(), 1).await.unwrap();
            assert!(!removed.linked);
            assert_eq!(removed.association_id, association.id);
            assert!(listed(library).await.is_empty());
            assert!(matches!(
                profile(library, &association.id).await,
                Err(Error::NotFound)
            ));
            let again = associate(library, &association.provider, provider::ARTIST_ID)
                .await
                .unwrap();
            assert_eq!(again.id, association.id);
            assert_eq!(revision(library).await, 3);
            assert_eq!(listed(library).await, vec![association.id.clone()]);
            assert!(matches!(
                profile(library, &association.id).await.unwrap(),
                ArtistProfileAvailability::Available { .. }
            ));
            let repeated = associate(library, &association.provider, provider::ARTIST_ID)
                .await
                .unwrap();
            assert_eq!(repeated.id, association.id);
            assert_eq!(
                revision(library).await,
                3,
                "an unchanged review is not a change"
            );
            assert!(matches!(
                undo(library, Uuid::new_v4(), 0).await,
                Err(Error::Conflict(_))
            ));
        }
        .boxed()
    })
    .await;
}

#[tokio::test]
async fn nothing_to_undo_and_foreign_accounts_are_rejected() {
    with_provider(|library, server| {
        async move {
            let url = server.url.clone();
            let association = library.review_artist(&url, provider::ARTIST_ID).await;
            let hito = &library.hito;
            let admin = &library.admin;
            assert!(matches!(
                hito.catalog
                    .undo_recording_artist_change(
                        admin,
                        &library.recording_id,
                        UndoRecordingArtistChange {
                            request_id: Uuid::new_v4(),
                            expected_revision: 0,
                        },
                    )
                    .await,
                Err(Error::Conflict(_))
            ));
            assert!(matches!(
                hito.catalog
                    .unlink_recording_artist(
                        admin,
                        &library.recording_id,
                        UnlinkRecordingArtist {
                            request_id: Uuid::new_v4(),
                            expected_revision: 0,
                            association_id: Uuid::parse_str(&association.id).unwrap(),
                        },
                    )
                    .await,
                Err(Error::NotFound)
            ));
            let admin_artist = hito
                .catalog
                .artists(admin, Browse::default())
                .await
                .unwrap()
                .items
                .remove(0);
            let observation = hito
                .catalog
                .recording_metadata(
                    admin,
                    &library.recording_id,
                    MetadataHistoryQuery::default(),
                )
                .await
                .unwrap()
                .selected_observation
                .unwrap();
            let settings = hito.catalog.metadata_settings(admin).await.unwrap();
            let own = hito
                .catalog
                .associate_recording_artist(
                    admin,
                    &library.recording_id,
                    AssociateRecordingArtist {
                        artist_id: admin_artist.id,
                        source_id: observation.source_id,
                        observation_id: observation.id,
                        role: ArtistCreditRole::RecordingArtist,
                        provider: MetadataProvider::BuiltinMusicBrainz,
                        provider_artist_id: "7e0f9d0a-6a4b-4b1c-9f55-2a8f0c2d7a11".into(),
                        policy_revision: settings.revision,
                    },
                )
                .await
                .unwrap();
            assert_eq!(revision(library).await, 1);
            unlink(library, Uuid::new_v4(), 1, &association.id)
                .await
                .unwrap();
            assert_eq!(
                hito.catalog
                    .recording_artist_associations(admin, &library.recording_id)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>(),
                vec![own.id.clone()],
                "another account keeps its own association"
            );
            assert_eq!(
                hito.catalog
                    .recording_artist_association_changes(admin, &library.recording_id)
                    .await
                    .unwrap()
                    .revision,
                1
            );
        }
        .boxed()
    })
    .await;
}

#[tokio::test]
async fn unlinking_keeps_other_associations_and_the_retention_cap() {
    with_provider(|library, server| {
        async move {
            let url = server.url.clone();
            let first = library.review_artist(&url, provider::ARTIST_ID).await;
            let mut ids = vec![first.id.clone()];
            for index in 1..32 {
                ids.push(
                    associate(library, &first.provider, &format!("artist-{index}"))
                        .await
                        .unwrap()
                        .id,
                );
            }
            assert!(matches!(
                associate(library, &first.provider, "artist-overflow").await,
                Err(Error::Conflict(_))
            ));
            assert_eq!(revision(library).await, 32);
            unlink(library, Uuid::new_v4(), 32, &first.id)
                .await
                .unwrap();
            assert_eq!(listed(library).await.len(), 31);
            assert!(!listed(library).await.contains(&first.id));
            associate(library, &first.provider, "artist-overflow")
                .await
                .unwrap();
            assert!(matches!(
                associate(library, &first.provider, "artist-second-overflow").await,
                Err(Error::Conflict(_))
            ));
            assert!(!matches!(
                profile(library, &ids[1]).await,
                Err(Error::NotFound)
            ));
        }
        .boxed()
    })
    .await;
}

#[tokio::test]
async fn association_changes_log_only_counts_and_outcomes() {
    with_provider(|library, server| {
        async move {
            let url = server.url.clone();
            let association = library.review_artist(&url, provider::ARTIST_ID).await;
            let captured = logs::CapturedLogs::default();
            let dispatch = tracing::Dispatch::new(captured.subscriber());
            async {
                let request = Uuid::new_v4();
                unlink(library, request, 1, &association.id).await.unwrap();
                unlink(library, request, 1, &association.id).await.unwrap();
                undo(library, Uuid::new_v4(), 2).await.unwrap();
                assert!(undo(library, Uuid::new_v4(), 0).await.is_err());
            }
            .with_subscriber(dispatch)
            .await;
            let text = captured.text();
            for private in [
                association.id.as_str(),
                library.recording_id.as_str(),
                library.artist_id.as_str(),
                provider::ARTIST_ID,
                library::CREDIT,
                url.as_str(),
            ] {
                assert!(!text.contains(private), "logs must omit private context");
            }
            for expected in [
                "artist_association_unlink",
                "artist_association_undo",
                "accepted",
                "replayed",
                "conflict",
            ] {
                assert!(text.contains(expected), "missing {expected}");
            }
        }
        .boxed()
    })
    .await;
}

#[tokio::test]
async fn unlinking_a_chosen_association_sends_the_identity_choice_back_to_review() {
    with_provider(|library, server| {
        async move {
            let association = library
                .review_artist(&server.url, provider::ARTIST_ID)
                .await;
            let context = library
                .hito
                .catalog
                .begin_artist_review(
                    &library.actor,
                    &library.recording_id,
                    association.provider.clone(),
                )
                .await
                .unwrap();
            library
                .hito
                .catalog
                .choose_recording_artist_identity(
                    &library.actor,
                    &library.recording_id,
                    ChooseRecordingArtistIdentity {
                        request_id: Uuid::new_v4(),
                        expected_revision: 0,
                        association_id: Uuid::parse_str(&association.id).unwrap(),
                        context,
                    },
                )
                .await
                .unwrap();
            unlink(library, Uuid::new_v4(), 1, &association.id)
                .await
                .unwrap();
            let choice = library
                .hito
                .catalog
                .recording_artist_identity_choice(&library.actor, &library.recording_id)
                .await
                .unwrap();
            assert!(matches!(
                choice.state,
                RecordingArtistIdentityState::Unselected
            ));
        }
        .boxed()
    })
    .await;
}

#[tokio::test]
async fn total_retained_rows_and_change_history_are_bounded() {
    with_provider(|library, server| {
        async move {
            let first = library
                .review_artist(&server.url, provider::ARTIST_ID)
                .await;
            for index in 1..128 {
                let association = associate(library, &first.provider, &format!("bounded-{index}"))
                    .await
                    .unwrap();
                let revision = revision(library).await;
                unlink(library, Uuid::new_v4(), revision, &association.id)
                    .await
                    .unwrap();
            }
            assert!(listed(library).await == vec![first.id.clone()]);
            assert!(matches!(
                associate(library, &first.provider, "bounded-overflow").await,
                Err(Error::Conflict(_))
            ));
            let mut accepted = revision(library).await;
            while accepted < 1024 {
                undo(library, Uuid::new_v4(), accepted).await.unwrap();
                accepted += 1;
            }
            assert!(matches!(
                undo(library, Uuid::new_v4(), accepted).await,
                Err(Error::Conflict(_))
            ));
            assert_eq!(revision(library).await, 1024);
        }
        .boxed()
    })
    .await;
}
