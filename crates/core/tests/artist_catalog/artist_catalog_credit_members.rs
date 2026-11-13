use futures_util::FutureExt;
use hito_core::test_support::{
    artist_catalog_discography_service as catalog, database_owner::DatabaseOwner,
    hang_guard::hang_guard, playlist_service as origin,
};
use hito_core::{
    Actor, ArtistAssociationEvidence, ArtistCreditRole, ArtistDiscographyAvailability,
    ArtistProfileAvailability, AssociateRecordingArtist, ChooseRecordingArtistIdentity, CoreConfig,
    Credentials, CreditRole, HostDeviceId, ImportProviderPlaylist, InstallPlugin,
    MetadataHistoryQuery, MetadataObservation, MetadataProvider, OfferPluginService,
    ReadArtistDiscography, RecordingArtistIdentityState, Services, UpdateMetadataSettings,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tokio::time::timeout;
use uuid::Uuid;

const ADA: &str = "owned:artist/ada";

struct Library {
    hito: Services,
    actor: Actor,
    owner: DatabaseOwner,
}

impl Library {
    async fn open() -> Self {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            directory.path().join("hito.db"),
            music,
            HostDeviceId::new("artist-credit-members").unwrap(),
        );
        owner.attach(&mut config);
        let hito = hito_core::open(config).await.unwrap();
        let session = hito
            .identity
            .setup(Credentials {
                username: "admin".into(),
                password: "owned artist credit members password".into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        Self { hito, actor, owner }
    }

    async fn close(self) {
        self.hito.playback.close().await.unwrap();
        self.hito.replicas.close().await.unwrap();
        self.hito.catalog_replication.close().await.unwrap();
        drop(self.hito);
        let retired = self.owner.retire(Duration::from_secs(5)).await;
        assert!(
            retired.resources_retired && !retired.deadline_exceeded,
            "SQLite workers must retire before the owned storage"
        );
        assert_eq!(self.owner.observe().active_resources(), 0);
    }
}

#[tokio::test]
async fn featured_member_review_preserves_the_main_choice_and_reads_its_own_catalog() {
    let library = hang_guard("the owned library must open", Box::pin(Library::open())).await;
    let origin = hang_guard(
        "the owned playlist provider must start",
        Box::pin(origin::ProviderServer::start()),
    )
    .await;
    let mut catalog = hang_guard(
        "the owned artist catalog must start",
        Box::pin(catalog::CatalogServer::start()),
    )
    .await;
    catalog.enable_discography();
    let outcome = AssertUnwindSafe(Box::pin(async {
        let prepared = hang_guard(
            "the structured credit and catalog must be prepared",
            Box::pin(prepare(&library, &origin, &catalog)),
        )
        .await;
        timeout(
            Duration::from_secs(30),
            Box::pin(exercise(&library, &prepared, &catalog)),
        )
        .await
    }))
    .catch_unwind()
    .await;
    let catalog_joined = timeout(Duration::from_secs(2), Box::pin(catalog.finish())).await;
    let origin_joined = timeout(Duration::from_secs(2), Box::pin(origin.shutdown())).await;
    Box::pin(library.close()).await;
    assert!(
        matches!(catalog_joined, Ok(true)),
        "the owned catalog must join shutdown"
    );
    assert!(
        origin_joined.is_ok(),
        "the owned playlist provider must join shutdown"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(_)) => panic!("the credited member catalog workflow exceeded its deadline"),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

struct Prepared {
    recording: String,
    playlist: Uuid,
    ada: String,
    cy: String,
    observation: MetadataObservation,
    provider: MetadataProvider,
    policy_revision: i64,
}

async fn prepare(
    library: &Library,
    origin: &origin::ProviderServer,
    catalog: &catalog::CatalogServer,
) -> Prepared {
    let hito = &library.hito;
    let actor = &library.actor;
    let installation = Box::pin(origin::connect(hito, actor, origin, &["playlists"])).await;
    origin::playlist(
        origin,
        "owned-credit",
        "owned-credit-v1",
        vec![json!({
            "occurrence_id": "owned-appearance",
            "item": {"kind": "recording", "recording": {
                "id": "owned-featured-recording", "title": "Owned duet",
                "artist": "Ada feat. Cy", "duration_ms": 100_000,
                "artists": [
                    {"name": "Ada", "role": "main", "join": " feat. "},
                    {"name": "Cy", "role": "featured"}
                ]
            }}
        })],
    )
    .await;
    let imported = Box::pin(hito.collection.import_provider_playlist(
        actor,
        &installation.id,
        ImportProviderPlaylist {
            request_id: Uuid::new_v4(),
            installation_revision: installation.revision,
            playlist_reference: "https://provider.invalid/owned-credit".into(),
        },
    ))
    .await
    .unwrap();
    let playlist = hito
        .collection
        .playlist(actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    let recording = playlist.entries[0].recording_id.as_deref().unwrap();
    let metadata = Box::pin(hito.catalog.recording_metadata(
        actor,
        recording,
        MetadataHistoryQuery::default(),
    ))
    .await
    .unwrap();
    let ada = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Ada")
        .unwrap();
    let cy = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Cy")
        .unwrap();
    assert_eq!(ada.role, CreditRole::Main);
    assert_eq!(cy.role, CreditRole::Featured);
    let ada = ada.artist_id.clone().unwrap();
    let cy = cy.artist_id.clone().unwrap();
    assert_ne!(ada, cy);
    let observation = metadata.selected_observation.unwrap();
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned credited member catalog".into(),
                base_url: catalog.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = Box::pin(hito.plugins.install(
        actor,
        InstallPlugin {
            accept_manifest_digest: None,
            service_id: service.id,
            service_revision: service.revision,
            bearer_token: None,
        },
    ))
    .await
    .unwrap();
    let provider = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let defaults = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    let settings = hito
        .artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: defaults.revision,
                enabled: true,
                providers: vec![provider.clone()],
            },
        )
        .await
        .unwrap();
    Prepared {
        recording: recording.into(),
        playlist: imported.playlist_id,
        ada,
        cy,
        observation,
        provider,
        policy_revision: settings.revision,
    }
}

async fn exercise(library: &Library, prepared: &Prepared, catalog: &catalog::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let recording = prepared.recording.as_str();
    let ada = prepared.ada.as_str();
    let cy = prepared.cy.as_str();
    let observation = &prepared.observation;
    let provider = &prepared.provider;
    let review = |artist: &str, identity: &str| AssociateRecordingArtist {
        artist_id: artist.into(),
        source_id: observation.source_id.clone(),
        observation_id: observation.id.clone(),
        role: ArtistCreditRole::RecordingArtist,
        provider: provider.clone(),
        provider_artist_id: identity.into(),
        policy_revision: prepared.policy_revision,
    };
    let main = Box::pin(hito.artist_catalog.associate_recording_artist(
        actor,
        recording,
        review(ada, ADA),
    ))
    .await
    .unwrap();
    let context = Box::pin(hito.artist_catalog.begin_artist_review(
        actor,
        recording,
        provider.clone(),
    ))
    .await
    .unwrap();
    hito.artist_catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                association_id: Uuid::parse_str(&main.id).unwrap(),
                context,
            },
        )
        .await
        .unwrap();
    let choice = hito
        .artist_catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    assert!(
        matches!(&choice.state, RecordingArtistIdentityState::Selected(selection) if selection.association_id == main.id)
    );
    let choice_before = serde_json::to_value(choice).unwrap();
    let before = Box::pin(snapshot(library, recording, prepared.playlist)).await;

    let guest = Box::pin(hito.artist_catalog.associate_recording_artist(
        actor,
        recording,
        review(cy, catalog::ARTIST_ID),
    ))
    .await
    .expect("a current featured member must authorize its own reviewed association");
    assert_eq!(guest.artist_id, cy);
    assert_eq!(guest.provider_artist_id, catalog::ARTIST_ID);
    assert_ne!(guest.id, main.id);
    assert!(matches!(
        guest.evidence,
        ArtistAssociationEvidence::UserConfirmed
    ));
    let profile = Box::pin(hito.artist_catalog.recording_artist_profile_until(
        actor,
        recording,
        &guest.id,
        pending(),
    ))
    .await
    .unwrap();
    let ArtistProfileAvailability::Available {
        association_id,
        profile,
    } = profile
    else {
        panic!("the guest's reviewed association must authorize its own profile");
    };
    assert_eq!(association_id, guest.id);
    assert!(
        profile == catalog::expected_profile(),
        "the guest profile must match the owned catalog response"
    );
    let discography = Box::pin(hito.artist_catalog.recording_artist_discography_until(
        actor,
        recording,
        &guest.id,
        ReadArtistDiscography {
            limit: 1,
            continuation: None,
            release_type: None,
        },
        pending(),
    ))
    .await
    .unwrap();
    let ArtistDiscographyAvailability::Available {
        association_id,
        page,
        ..
    } = discography
    else {
        panic!("the guest's reviewed association must authorize its own discography");
    };
    assert_eq!(association_id, guest.id);
    assert!(
        page == catalog::expected_page(false),
        "the guest discography must match the owned catalog response"
    );
    let guest_page = Box::pin(hito.catalog.artist(actor, cy, Default::default()))
        .await
        .unwrap();
    assert!(guest_page.artist.member_only);
    assert_eq!(guest_page.recordings.total, 0);
    assert_eq!(guest_page.featured_recordings.total, 1);
    assert_eq!(guest_page.featured_recordings.items[0].id, recording);
    assert_eq!(
        serde_json::to_value(
            hito.artist_catalog
                .recording_artist_identity_choice(actor, recording)
                .await
                .unwrap()
        )
        .unwrap(),
        choice_before
    );
    assert_eq!(
        hito.artist_catalog
            .artist_identity_groups(actor, Default::default())
            .await
            .unwrap()
            .total,
        1,
        "reviewing Cy stores evidence without selecting Cy or replacing Ada"
    );
    assert_eq!(
        Box::pin(snapshot(library, recording, prepared.playlist)).await,
        before
    );
    assert_eq!(catalog.unexpected(), 0);
}

async fn snapshot(library: &Library, recording: &str, playlist: Uuid) -> Value {
    let hito = &library.hito;
    let actor = &library.actor;
    json!({
        "recordings": hito.catalog.recordings_by_ids(actor, &[Uuid::parse_str(recording).unwrap()]).await.unwrap(),
        "metadata": Box::pin(hito.catalog.recording_metadata(actor, recording, MetadataHistoryQuery::default())).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "playlist": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "progress": hito.collection.saved_playback(actor, &[Uuid::parse_str(recording).unwrap()]).await.unwrap()
    })
}
