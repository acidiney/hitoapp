#[allow(dead_code)]
#[path = "../fixtures/artist_catalog_profiles_library.rs"]
mod library;
use hito_core::test_support::artist_catalog_profiles_service as provider;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{
    Action, ArtistAssociationEvidence, ArtistCreditRole, ArtistProfileAvailability,
    AssociateRecordingArtist, Browse, MetadataHistoryQuery,
};
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

#[tokio::test]
async fn rereviewing_a_changed_credit_keeps_each_review_attached_to_its_own_artist() {
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

async fn exercise(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let original = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let original_evidence = serde_json::to_value(&original).unwrap();
    let metadata = hito
        .catalog
        .recording_metadata(
            actor,
            &library.recording_id,
            MetadataHistoryQuery::default(),
        )
        .await
        .unwrap();
    let observed_before = serde_json::to_value(metadata.selected_observation).unwrap();
    let sources_before = serde_json::to_value(
        hito.catalog
            .sources(actor, &library.recording_id)
            .await
            .unwrap(),
    )
    .unwrap();
    let changed_credit = "Private reviewed performer credit";
    support::synchronize(
        hito,
        actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&library.recording_id).unwrap(),
            artist: changed_credit.into(),
        }],
    )
    .await;
    let metadata = hito
        .catalog
        .recording_metadata(
            actor,
            &library.recording_id,
            MetadataHistoryQuery::default(),
        )
        .await
        .unwrap();
    assert!(
        metadata.artist.value == changed_credit,
        "The public artist edit must be effective"
    );
    assert!(
        metadata.artist.personal.as_deref() == Some(changed_credit),
        "The artist edit must remain a personal pin"
    );
    assert!(
        serde_json::to_value(metadata.selected_observation).unwrap() == observed_before,
        "Changing the personal credit must preserve its immutable source observation"
    );
    assert!(
        serde_json::to_value(
            hito.catalog
                .sources(actor, &library.recording_id)
                .await
                .unwrap()
        )
        .unwrap()
            == sources_before,
        "Changing the personal credit must preserve its sources"
    );
    let artist = hito
        .catalog
        .artists(actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|artist| artist.name == changed_credit)
        .expect("The changed personal credit must have its own account artist");
    assert!(
        artist.id != original.artist_id,
        "Different exact credits must retain distinct local artists"
    );
    let after_edit = library.snapshot().await;
    let settings = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    let reviewed = hito
        .artist_catalog
        .associate_recording_artist(
            actor,
            &library.recording_id,
            AssociateRecordingArtist {
                artist_id: artist.id.clone(),
                source_id: Some(library.source_id.clone()),
                observation_id: library.observation_id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: original.provider.clone(),
                provider_artist_id: provider::ARTIST_ID.into(),
                policy_revision: settings.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        reviewed.artist_id == artist.id,
        "Re-review must return the current artist credit rather than an earlier immutable association"
    );
    assert!(
        reviewed.id != original.id,
        "A different reviewed credit needs distinct association evidence"
    );
    assert!(
        reviewed.observation_id == original.observation_id,
        "Both reviews must retain the same observed source evidence"
    );
    assert!(
        reviewed.evidence == ArtistAssociationEvidence::UserConfirmed,
        "Re-review must retain explicit user-confirmed provenance"
    );
    let result = hito
        .artist_catalog
        .recording_artist_profile_until(
            actor,
            &library.recording_id,
            &reviewed.id,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistProfileAvailability::Available {
        association_id,
        profile,
    } = result
    else {
        panic!("The current reviewed credit must provide its exact artist profile");
    };
    assert!(
        association_id == reviewed.id,
        "The profile must refer to the current reviewed association"
    );
    assert!(profile == provider::expected_profile());
    let associations = hito
        .artist_catalog
        .recording_artist_associations(actor, &library.recording_id)
        .await
        .unwrap();
    assert!(
        associations.len() == 2,
        "Both distinct reviewed credits must be retained"
    );
    let historical = associations
        .iter()
        .find(|association| association.id == original.id)
        .expect("The earlier immutable review must remain available as evidence");
    assert!(
        serde_json::to_value(historical).unwrap() == original_evidence,
        "Re-review must not rewrite the earlier association"
    );
    let associations_before_repeat = serde_json::to_value(&associations).unwrap();
    let requests_before_repeat = server.counts().await;
    let repeated = hito
        .artist_catalog
        .associate_recording_artist(
            actor,
            &library.recording_id,
            AssociateRecordingArtist {
                artist_id: artist.id.clone(),
                source_id: Some(library.source_id.clone()),
                observation_id: library.observation_id.clone(),
                role: ArtistCreditRole::RecordingArtist,
                provider: original.provider.clone(),
                provider_artist_id: provider::ARTIST_ID.into(),
                policy_revision: settings.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        repeated.id == reviewed.id,
        "An identical current-credit review must retain its association ID"
    );
    assert!(
        serde_json::to_value(&repeated).unwrap() == serde_json::to_value(&reviewed).unwrap(),
        "An identical current-credit review must retain its exact immutable evidence"
    );
    let associations_after_repeat = hito
        .artist_catalog
        .recording_artist_associations(actor, &library.recording_id)
        .await
        .unwrap();
    assert!(
        associations_after_repeat.len() == 2,
        "Repeating the current review must not append another association"
    );
    assert!(
        serde_json::to_value(associations_after_repeat).unwrap() == associations_before_repeat,
        "Repeating the current review must preserve both earlier and current association evidence"
    );
    assert_eq!(server.counts().await, requests_before_repeat);
    assert!(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.other_recording_id,)
            .await
            .unwrap()
            .is_empty(),
        "The other recording must not inherit either reviewed association"
    );
    assert!(
        library.snapshot().await == after_edit,
        "Artist review must preserve the edited metadata, sources, recordings and private collection state"
    );
    assert!(
        server.counts().await.unexpected == 0,
        "The provider must receive only supported catalog operations"
    );
}
