use futures_util::FutureExt;
use hito_core::test_support::{
    database_owner::DatabaseOwner, hang_guard::hang_guard, playlist_service as origin,
};
use hito_core::{
    Actor, ArtistAssociation, ArtistCreditRole, AssociateRecordingArtist,
    ChooseRecordingArtistIdentity, CoreConfig, Credentials, CreditRole, HostDeviceId,
    ImportProviderPlaylist, MetadataHistoryQuery, MetadataProvider, PromoteArtistCorrection,
    PromotionSource, RecordingArtistIdentityState, Services, UnlinkRecordingArtist,
    UpdateMetadataSettings,
};
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tokio::time::timeout;
use uuid::Uuid;

const ADA: &str = "00000000-0000-4000-8000-0000000000a1";
const CY: &str = "00000000-0000-4000-8000-0000000000c1";
const SOLO: &str = "00000000-0000-4000-8000-000000000051";

#[tokio::test]
async fn named_member_unlink_is_not_offered_as_shareable() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("member-shareability").unwrap(),
    );
    owner.attach(&mut config);
    let hito = hang_guard(
        "the owned library must open",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let origin = hang_guard(
        "the owned playlist provider must start",
        Box::pin(origin::ProviderServer::start()),
    )
    .await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let session = hang_guard(
            "the owner must be created",
            Box::pin(hito.identity.setup(Credentials {
                username: "owner".into(),
                password: "owned member shareability password".into(),
            })),
        )
        .await
        .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let prepared = hang_guard(
            "the owned credit reviews must be prepared",
            Box::pin(prepare(&hito, &actor, &origin)),
        )
        .await;
        timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&hito, &actor, &prepared)),
        )
        .await
    }))
    .catch_unwind()
    .await;
    let joined = timeout(Duration::from_secs(2), Box::pin(origin.shutdown())).await;
    hito.playback.close().await.unwrap();
    hito.replicas.close().await.unwrap();
    hito.catalog_replication.close().await.unwrap();
    drop(hito);
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert!(
        joined.is_ok(),
        "the owned playlist provider must join shutdown"
    );
    let shareable = match outcome {
        Ok(Ok(shareable)) => shareable,
        Ok(Err(_)) => panic!("the member shareability workflow exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        !shareable,
        "a named-member unlink must not be offered as a shareable whole-credit correction"
    );
}

struct Prepared {
    recording: String,
    solo: String,
    playlist: Uuid,
    guest: ArtistAssociation,
    whole: ArtistAssociation,
    main_choice: Value,
    private_state: Value,
}

async fn prepare(hito: &Services, actor: &Actor, server: &origin::ProviderServer) -> Prepared {
    let installation = Box::pin(origin::connect(hito, actor, server, &["playlists"])).await;
    origin::playlist(server, "owned-members", "owned-members-v1", vec![
        json!({"occurrence_id":"duet", "item":{"kind":"recording","recording":{
            "id":"owned-duet","title":"Owned duet","artist":"Ada feat. Cy","duration_ms":100000,
            "artists":[{"name":"Ada","role":"main","join":" feat. "},{"name":"Cy","role":"featured"}]
        }}}),
        json!({"occurrence_id":"solo", "item":{"kind":"recording","recording":{
            "id":"owned-solo","title":"Owned solo","artist":"Solo","duration_ms":90000
        }}}),
    ]).await;
    let imported = Box::pin(hito.collection.import_provider_playlist(
        actor,
        &installation.id,
        ImportProviderPlaylist {
            request_id: Uuid::new_v4(),
            installation_revision: installation.revision,
            playlist_reference: "https://provider.invalid/owned-members".into(),
        },
    ))
    .await
    .unwrap();
    let playlist = hito
        .collection
        .playlist(actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    let recording = playlist.entries[0].recording_id.clone().unwrap();
    let solo = playlist.entries[1].recording_id.clone().unwrap();
    assert_ne!(recording, solo);
    let defaults = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    let settings = hito
        .artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: defaults.revision,
                enabled: true,
                providers: vec![MetadataProvider::BuiltinMusicBrainz],
            },
        )
        .await
        .unwrap();
    let metadata = Box::pin(hito.catalog.recording_metadata(
        actor,
        &recording,
        MetadataHistoryQuery::default(),
    ))
    .await
    .unwrap();
    let observation = metadata.selected_observation.unwrap();
    let ada = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Ada")
        .unwrap()
        .artist_id
        .as_deref()
        .unwrap();
    let member = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Cy")
        .unwrap();
    assert_eq!(member.role, CreditRole::Featured);
    let cy = member.artist_id.as_deref().unwrap();
    assert_ne!(ada, cy);
    let request = |artist: &str, identity: &str| AssociateRecordingArtist {
        artist_id: artist.into(),
        source_id: observation.source_id.clone(),
        observation_id: observation.id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: MetadataProvider::BuiltinMusicBrainz,
        provider_artist_id: identity.into(),
        policy_revision: settings.revision,
    };
    let main = Box::pin(hito.artist_catalog.associate_recording_artist(
        actor,
        &recording,
        request(ada, ADA),
    ))
    .await
    .unwrap();
    let context = Box::pin(hito.artist_catalog.begin_artist_review(
        actor,
        &recording,
        MetadataProvider::BuiltinMusicBrainz,
    ))
    .await
    .unwrap();
    hito.artist_catalog
        .choose_recording_artist_identity(
            actor,
            &recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                association_id: Uuid::parse_str(&main.id).unwrap(),
                context,
            },
        )
        .await
        .unwrap();
    let guest = Box::pin(hito.artist_catalog.associate_recording_artist(
        actor,
        &recording,
        request(cy, CY),
    ))
    .await
    .unwrap();
    assert_eq!(guest.artist_id, cy);
    assert_eq!(guest.provider, MetadataProvider::BuiltinMusicBrainz);
    let metadata = Box::pin(hito.catalog.recording_metadata(
        actor,
        &solo,
        MetadataHistoryQuery::default(),
    ))
    .await
    .unwrap();
    let observation = metadata.selected_observation.unwrap();
    let solo_context = Box::pin(hito.artist_catalog.begin_artist_review(
        actor,
        &solo,
        MetadataProvider::BuiltinMusicBrainz,
    ))
    .await
    .unwrap();
    let whole = Box::pin(hito.artist_catalog.associate_recording_artist(
        actor,
        &solo,
        AssociateRecordingArtist {
            artist_id: solo_context.artist_id,
            source_id: observation.source_id,
            observation_id: observation.id,
            role: ArtistCreditRole::RecordingArtist,
            provider: MetadataProvider::BuiltinMusicBrainz,
            provider_artist_id: SOLO.into(),
            policy_revision: settings.revision,
        },
    ))
    .await
    .unwrap();
    let choice = hito
        .artist_catalog
        .recording_artist_identity_choice(actor, &recording)
        .await
        .unwrap();
    assert!(
        matches!(&choice.state, RecordingArtistIdentityState::Selected(selection) if selection.association_id == main.id)
    );
    let private_state = Box::pin(snapshot(
        hito,
        actor,
        &recording,
        &solo,
        imported.playlist_id,
    ))
    .await;
    Prepared {
        recording,
        solo,
        playlist: imported.playlist_id,
        guest,
        whole,
        main_choice: serde_json::to_value(choice).unwrap(),
        private_state,
    }
}

async fn exercise(hito: &Services, actor: &Actor, prepared: &Prepared) -> bool {
    let whole_changes = hito
        .artist_catalog
        .recording_artist_association_changes(actor, &prepared.solo)
        .await
        .unwrap();
    let unlinked = hito
        .artist_catalog
        .unlink_recording_artist(
            actor,
            &prepared.solo,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: whole_changes.revision,
                association_id: Uuid::parse_str(&prepared.whole.id).unwrap(),
            },
        )
        .await
        .unwrap();
    assert!(!unlinked.linked);
    let whole = hito
        .artist_catalog
        .recording_artist_association_changes(actor, &prepared.solo)
        .await
        .unwrap();
    assert!(
        whole.shareable,
        "a builtin whole-credit unlink must remain shareable"
    );
    let guest_changes = hito
        .artist_catalog
        .recording_artist_association_changes(actor, &prepared.recording)
        .await
        .unwrap();
    let unlinked = hito
        .artist_catalog
        .unlink_recording_artist(
            actor,
            &prepared.recording,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: guest_changes.revision,
                association_id: Uuid::parse_str(&prepared.guest.id).unwrap(),
            },
        )
        .await
        .unwrap();
    assert!(!unlinked.linked);
    let guest = hito
        .artist_catalog
        .recording_artist_association_changes(actor, &prepared.recording)
        .await
        .unwrap();
    assert_eq!(guest.revision, unlinked.revision);
    assert_eq!(
        guest.undo.as_ref().unwrap().association_id,
        prepared.guest.id
    );
    assert!(guest.undo.as_ref().unwrap().linked);
    assert!(guest.shared.is_none());
    let refused = hito
        .artist_catalog
        .promote_artist_correction(
            actor,
            PromoteArtistCorrection {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                source: PromotionSource::Unlink {
                    recording_id: prepared.recording.clone(),
                    association_id: Uuid::parse_str(&prepared.guest.id).unwrap(),
                },
            },
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code(), "not_found");
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_artist_identity_choice(actor, &prepared.recording)
                .await
                .unwrap()
        )
        .unwrap(),
        prepared.main_choice
    );
    assert_eq!(
        Box::pin(snapshot(
            hito,
            actor,
            &prepared.recording,
            &prepared.solo,
            prepared.playlist
        ))
        .await,
        prepared.private_state
    );
    guest.shareable
}

async fn snapshot(
    hito: &Services,
    actor: &Actor,
    recording: &str,
    solo: &str,
    playlist: Uuid,
) -> Value {
    let ids = [
        Uuid::parse_str(recording).unwrap(),
        Uuid::parse_str(solo).unwrap(),
    ];
    json!({
        "recordings": hito.catalog.recordings_by_ids(actor, &ids).await.unwrap(),
        "metadata": [
            Box::pin(hito.catalog.recording_metadata(actor, recording, MetadataHistoryQuery::default())).await.unwrap(),
            Box::pin(hito.catalog.recording_metadata(actor, solo, MetadataHistoryQuery::default())).await.unwrap()
        ],
        "sources": [hito.catalog.sources(actor, recording).await.unwrap(), hito.catalog.sources(actor, solo).await.unwrap()],
        "playlist": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "progress": hito.collection.saved_playback(actor, &ids).await.unwrap(),
        "plugins": hito.plugins.settings(actor).await.unwrap(),
        "shared_corrections": hito.artist_catalog.shared_artist_corrections(actor).await.unwrap()
    })
}
