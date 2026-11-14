use super::super::artist_catalog_credit_members::{Library, Prepared, prepare};
use futures_util::FutureExt;
use hito_core::{
    Action, ChooseCreditMemberIdentity, CoreConfig, HostDeviceId, Operation, SyncBatch,
    test_support::{
        artist_catalog_discography_service as catalog, database_owner::DatabaseOwner,
        hang_guard::hang_guard, playlist_service as origin,
    },
};
use serde_json::{Value, json};
use std::{future::Future, panic::AssertUnwindSafe, sync::Arc, time::Duration};

pub(super) struct RepairFixture {
    pub(super) library: Option<Library>,
    pub(super) origin: Option<origin::ProviderServer>,
    pub(super) catalog: Option<catalog::CatalogServer>,
    pub(super) directory: Option<Arc<tempfile::TempDir>>,
    pub(super) token: Option<String>,
    pub(super) prepared: Option<Prepared>,
    pub(super) selected: Option<SelectedCollaboration>,
    pub(super) singleton: Option<Singleton>,
    pub(super) registration: Option<Registration>,
    pub(super) override_case: Option<PrivateOverride>,
    pub(super) services: Option<hito_core::Services>,
    pub(super) owner: Option<DatabaseOwner>,
    cleanup_outcomes: Vec<CleanupOutcome>,
}

pub(super) struct SelectedCollaboration {
    pub(super) recording: String,
    pub(super) main: String,
    pub(super) guest: String,
    pub(super) guest_request_id: Option<uuid::Uuid>,
    pub(super) override_title: String,
    pub(super) guest_association: String,
    pub(super) private_snapshot: Option<Value>,
    pub(super) guest_revision: i64,
}

pub(super) struct Singleton {
    pub(super) recording: String,
    pub(super) member: String,
    pub(super) credit_snapshot: Option<Value>,
}

pub(super) struct Registration {
    pub(super) anchor_recording: String,
    pub(super) anchor: String,
    pub(super) recording: String,
    pub(super) main: String,
    pub(super) guest: String,
    pub(super) repaired_main: Option<String>,
    pub(super) credit_snapshot: Option<Value>,
}

pub(super) struct PrivateOverride {
    pub(super) recording: String,
    pub(super) guest: String,
    pub(super) request_id: Option<String>,
    pub(super) private_snapshot: Option<Value>,
}

struct OwnedRecordingSpec<'a> {
    playlist_id: &'a str,
    snapshot: &'a str,
    occurrence: &'a str,
    recording_id: &'a str,
    title: &'a str,
    artist: &'a str,
    artists: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShutdownAttempt {
    Closed,
    Failed,
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetirementAttempt {
    Retired,
    DeadlineExceeded,
    ResourcesRemain,
    Panicked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderAttempt {
    Joined,
    Failed,
    Panicked,
}

struct RetirementOutcome {
    attempt: RetirementAttempt,
    active_resources: usize,
}

#[derive(Default)]
struct CoreShutdownOutcome {
    playback: Option<ShutdownAttempt>,
    replicas: Option<ShutdownAttempt>,
    catalog_replication: Option<ShutdownAttempt>,
    retirement: Option<RetirementOutcome>,
}

#[derive(Default)]
struct CleanupOutcome {
    core: Option<CoreShutdownOutcome>,
    catalog_provider: Option<ProviderAttempt>,
    playlist_provider: Option<ProviderAttempt>,
}

impl RepairFixture {
    pub(super) fn new() -> Self {
        Self {
            library: None,
            origin: None,
            catalog: None,
            directory: None,
            token: None,
            prepared: None,
            selected: None,
            singleton: None,
            registration: None,
            override_case: None,
            services: None,
            owner: None,
            cleanup_outcomes: Vec::new(),
        }
    }

    pub(super) async fn start(&mut self) {
        self.library =
            Some(hang_guard("the owned library must open", Box::pin(Library::open())).await);
        self.owner = Some(self.library.as_ref().unwrap().owner.clone());
        self.origin = Some(
            hang_guard(
                "the owned playlist provider must start",
                Box::pin(origin::ProviderServer::start()),
            )
            .await,
        );
        self.catalog = Some(
            hang_guard(
                "the owned artist catalog must start",
                Box::pin(catalog::CatalogServer::start()),
            )
            .await,
        );
        self.catalog.as_mut().unwrap().enable_discography();
        let library = self.library.as_ref().unwrap();
        self.directory = Some(library.directory.clone());
        self.token = Some(library.token.clone());
    }

    pub(super) async fn prepare(&mut self) {
        let prepared = hang_guard(
            "the structured credit must be prepared",
            Box::pin(prepare(
                self.library.as_ref().unwrap(),
                self.origin.as_ref().unwrap(),
                self.catalog.as_ref().unwrap(),
            )),
        )
        .await;
        self.prepared = Some(prepared);
        let installation = &self.prepared.as_ref().unwrap().playlist_installation;
        let singleton = import_singleton(
            self.library.as_ref().unwrap(),
            self.origin.as_ref().unwrap(),
            installation,
        )
        .await;
        self.singleton = Some(Singleton {
            recording: singleton.0,
            member: singleton.1,
            credit_snapshot: None,
        });
        let collaboration = import_collaboration(
            self.library.as_ref().unwrap(),
            self.origin.as_ref().unwrap(),
            installation,
        )
        .await;
        let registration = import_registration_case(
            self.library.as_ref().unwrap(),
            self.origin.as_ref().unwrap(),
            installation,
        )
        .await;
        self.registration = Some(Registration {
            anchor_recording: registration.0,
            anchor: registration.1,
            recording: registration.2,
            main: registration.3,
            guest: registration.4,
            repaired_main: None,
            credit_snapshot: None,
        });
        self.override_case = Some(PrivateOverride {
            recording: collaboration.0,
            guest: collaboration.1,
            request_id: None,
            private_snapshot: None,
        });
        let prepared = self.prepared.as_ref().unwrap();
        self.selected = Some(SelectedCollaboration {
            recording: prepared.recording.clone(),
            main: prepared.ada.clone(),
            guest: prepared.cy.clone(),
            guest_request_id: None,
            override_title: "Owned private repair title".into(),
            guest_association: String::new(),
            private_snapshot: None,
            guest_revision: 0,
        });
    }

    pub(super) async fn prepare_private_override(&mut self) {
        let override_case = self.override_case.as_ref().unwrap();
        let prepared = self.prepared.as_ref().unwrap();
        let context = self
            .library
            .as_ref()
            .unwrap()
            .hito
            .artist_catalog
            .begin_credit_member_review(
                &self.library.as_ref().unwrap().actor,
                &override_case.recording,
                &override_case.guest,
                prepared.provider.clone(),
            )
            .await
            .unwrap();
        let association = self
            .library
            .as_ref()
            .unwrap()
            .hito
            .artist_catalog
            .confirm_credit_member_review(
                &self.library.as_ref().unwrap().actor,
                &context,
                catalog::ARTIST_ID,
            )
            .await
            .unwrap();
        let request_id = uuid::Uuid::new_v4();
        self.library
            .as_ref()
            .unwrap()
            .hito
            .artist_catalog
            .choose_credit_member_identity(
                &self.library.as_ref().unwrap().actor,
                &override_case.recording,
                &override_case.guest,
                ChooseCreditMemberIdentity {
                    request_id,
                    expected_revision: 0,
                    association_id: uuid::Uuid::parse_str(&association.id).unwrap(),
                    context,
                },
            )
            .await
            .unwrap();
        self.library
            .as_ref()
            .unwrap()
            .hito
            .collection
            .apply(
                &self.library.as_ref().unwrap().actor,
                SyncBatch {
                    device_id: "member139-repair".into(),
                    operations: vec![Operation {
                        id: uuid::Uuid::new_v4(),
                        action: Action::RecordingArtistSet {
                            recording_id: uuid::Uuid::parse_str(&override_case.recording).unwrap(),
                            artist: "Private artist override".into(),
                        },
                    }],
                },
            )
            .await
            .unwrap();
        self.override_case.as_mut().unwrap().request_id = Some(request_id.to_string());
    }

    pub(super) async fn prepare_shared_choices(&mut self) {
        let library = self.library.as_ref().unwrap();
        let prepared = self.prepared.as_ref().unwrap();
        let selected = self.selected.as_mut().unwrap();
        selected.guest_association = super::super::artist_catalog_credit_members::exercise(
            library,
            prepared,
            self.catalog.as_ref().unwrap(),
        )
        .await;
        library
            .hito
            .collection
            .apply(
                &library.actor,
                SyncBatch {
                    device_id: "member139-repair".into(),
                    operations: vec![Operation {
                        id: uuid::Uuid::new_v4(),
                        action: Action::RecordingTitleSet {
                            recording_id: uuid::Uuid::parse_str(&selected.recording).unwrap(),
                            title: selected.override_title.clone(),
                        },
                    }],
                },
            )
            .await
            .unwrap();
        let context = library
            .hito
            .artist_catalog
            .begin_credit_member_review(
                &library.actor,
                &selected.recording,
                &selected.guest,
                prepared.provider.clone(),
            )
            .await
            .unwrap();
        let request_id = uuid::Uuid::new_v4();
        selected.guest_request_id = Some(request_id);
        let receipt = library
            .hito
            .artist_catalog
            .choose_credit_member_identity(
                &library.actor,
                &selected.recording,
                &selected.guest,
                ChooseCreditMemberIdentity {
                    request_id,
                    expected_revision: 0,
                    association_id: uuid::Uuid::parse_str(&selected.guest_association).unwrap(),
                    context,
                },
            )
            .await
            .unwrap();
        selected.guest_revision = receipt.revision;
    }

    pub(super) async fn close_initial(&mut self) {
        let library = self.library.as_ref().unwrap();
        let playback = shutdown_attempt(library.hito.playback.close()).await;
        let replicas = shutdown_attempt(library.hito.replicas.close()).await;
        let catalog_replication = shutdown_attempt(library.hito.catalog_replication.close()).await;
        drop(self.library.take());
        let retirement = retirement_attempt(self.owner.as_ref().unwrap()).await;
        let outcome = CoreShutdownOutcome {
            playback: Some(playback),
            replicas: Some(replicas),
            catalog_replication: Some(catalog_replication),
            retirement: Some(retirement),
        };
        self.cleanup_outcomes.push(CleanupOutcome {
            core: Some(outcome),
            ..CleanupOutcome::default()
        });
        assert_core_shutdown(self.cleanup_outcomes.last().unwrap().core.as_ref().unwrap());
        drop(self.owner.take());
    }

    pub(super) async fn reopen(&mut self) {
        let directory = self.directory.as_ref().unwrap();
        let database = directory.path().join("hito.db");
        let music = directory.path().join("music");
        let owner = DatabaseOwner::retain(directory.clone());
        let mut config = CoreConfig::new(
            database,
            music,
            HostDeviceId::new("artist-credit-members").unwrap(),
        );
        owner.attach(&mut config);
        self.owner = Some(owner);
        self.services = Some(hito_core::open(config).await.unwrap());
    }

    pub(super) async fn close_reopened(&mut self) {
        let services = self.services.as_ref().unwrap();
        let playback = shutdown_attempt(services.playback.close()).await;
        let replicas = shutdown_attempt(services.replicas.close()).await;
        let catalog_replication = shutdown_attempt(services.catalog_replication.close()).await;
        drop(self.services.take());
        let retirement = retirement_attempt(self.owner.as_ref().unwrap()).await;
        let outcome = CoreShutdownOutcome {
            playback: Some(playback),
            replicas: Some(replicas),
            catalog_replication: Some(catalog_replication),
            retirement: Some(retirement),
        };
        self.cleanup_outcomes.push(CleanupOutcome {
            core: Some(outcome),
            ..CleanupOutcome::default()
        });
        assert_core_shutdown(self.cleanup_outcomes.last().unwrap().core.as_ref().unwrap());
        drop(self.owner.take());
    }

    pub(super) async fn finish(&mut self) {
        let mut outcome = CleanupOutcome::default();
        let mut core = CoreShutdownOutcome::default();
        if let Some(library) = self.library.as_ref() {
            core.playback = Some(shutdown_attempt(library.hito.playback.close()).await);
            core.replicas = Some(shutdown_attempt(library.hito.replicas.close()).await);
            core.catalog_replication =
                Some(shutdown_attempt(library.hito.catalog_replication.close()).await);
            drop(self.library.take());
        }
        if let Some(services) = self.services.as_ref() {
            core.playback = Some(shutdown_attempt(services.playback.close()).await);
            core.replicas = Some(shutdown_attempt(services.replicas.close()).await);
            core.catalog_replication =
                Some(shutdown_attempt(services.catalog_replication.close()).await);
            drop(self.services.take());
        }
        if let Some(owner) = self.owner.as_ref() {
            core.retirement = Some(retirement_attempt(owner).await);
        }
        if core.playback.is_some()
            || core.replicas.is_some()
            || core.catalog_replication.is_some()
            || core.retirement.is_some()
        {
            outcome.core = Some(core);
        }
        if let Some(catalog) = self.catalog.as_mut() {
            outcome.catalog_provider = Some(
                match AssertUnwindSafe(Box::pin(catalog.finish()))
                    .catch_unwind()
                    .await
                {
                    Ok(true) => ProviderAttempt::Joined,
                    Ok(false) => ProviderAttempt::Failed,
                    Err(_) => ProviderAttempt::Panicked,
                },
            );
            drop(self.catalog.take());
        }
        if let Some(origin) = self.origin.take() {
            outcome.playlist_provider = Some(
                match AssertUnwindSafe(Box::pin(origin.shutdown()))
                    .catch_unwind()
                    .await
                {
                    Ok(()) => ProviderAttempt::Joined,
                    Err(_) => ProviderAttempt::Panicked,
                },
            );
        }
        self.cleanup_outcomes.push(outcome);
        for outcome in &self.cleanup_outcomes {
            if let Some(core) = outcome.core.as_ref() {
                assert_core_shutdown(core);
            }
            if let Some(catalog) = outcome.catalog_provider {
                assert_eq!(catalog, ProviderAttempt::Joined);
            }
            if let Some(origin) = outcome.playlist_provider {
                assert_eq!(origin, ProviderAttempt::Joined);
            }
        }
        if self.owner.is_some()
            && self
                .cleanup_outcomes
                .last()
                .and_then(|outcome| outcome.core.as_ref())
                .and_then(|core| core.retirement.as_ref())
                .is_some_and(|retirement| retirement.attempt == RetirementAttempt::Retired)
        {
            drop(self.owner.take());
        }
    }
}

async fn shutdown_attempt<F, E>(future: F) -> ShutdownAttempt
where
    F: Future<Output = Result<(), E>>,
{
    match AssertUnwindSafe(Box::pin(future)).catch_unwind().await {
        Ok(Ok(())) => ShutdownAttempt::Closed,
        Ok(Err(_)) => ShutdownAttempt::Failed,
        Err(_) => ShutdownAttempt::Panicked,
    }
}

async fn retirement_attempt(owner: &DatabaseOwner) -> RetirementOutcome {
    let result = AssertUnwindSafe(Box::pin(owner.retire(Duration::from_secs(5))))
        .catch_unwind()
        .await;
    let active_resources = owner.observe().active_resources();
    let attempt = match result {
        Ok(retired) if retired.resources_retired && !retired.deadline_exceeded => {
            RetirementAttempt::Retired
        }
        Ok(retired) if retired.deadline_exceeded => RetirementAttempt::DeadlineExceeded,
        Ok(_) => RetirementAttempt::ResourcesRemain,
        Err(_) => RetirementAttempt::Panicked,
    };
    RetirementOutcome {
        attempt,
        active_resources,
    }
}

fn assert_core_shutdown(outcome: &CoreShutdownOutcome) {
    for attempt in [
        outcome.playback,
        outcome.replicas,
        outcome.catalog_replication,
    ]
    .into_iter()
    .flatten()
    {
        assert_eq!(attempt, ShutdownAttempt::Closed);
    }
    if let Some(retirement) = outcome.retirement.as_ref() {
        assert_eq!(retirement.attempt, RetirementAttempt::Retired);
        assert_eq!(retirement.active_resources, 0);
    }
}

async fn import_singleton(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
) -> (String, String) {
    let recording = import_owned_recording(
        library,
        origin,
        installation,
        OwnedRecordingSpec {
            playlist_id: "owned-singleton-credit",
            snapshot: "owned-singleton-credit-v1",
            occurrence: "owned-singleton-appearance",
            recording_id: "owned-singleton-recording",
            title: "Owned singleton",
            artist: "Owned singleton collection",
            artists: json!([{"name": "Owned singleton main", "role": "main"}]),
        },
    )
    .await;
    let metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &recording, Default::default())
        .await
        .unwrap();
    let member = metadata
        .credited_artists
        .iter()
        .find(|member| {
            member.name == "Owned singleton main"
                && member.role == hito_plugin_sdk::CreditRole::Main
        })
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    (recording, member)
}

async fn import_registration_case(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
) -> (String, String, String, String, String) {
    let anchor_recording = import_owned_recording(
        library,
        origin,
        installation,
        OwnedRecordingSpec {
            playlist_id: "owned-registration-anchor",
            snapshot: "owned-registration-anchor-v1",
            occurrence: "owned-registration-anchor-appearance",
            recording_id: "owned-registration-anchor-recording",
            title: "Owned registration anchor",
            artist: "Owned registration anchor",
            artists: json!([{"name": "Owned registration anchor", "role": "main"}]),
        },
    )
    .await;
    let anchor_metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &anchor_recording, Default::default())
        .await
        .unwrap();
    let anchor_id = anchor_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Owned registration anchor")
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    let registration_recording = import_owned_recording(
        library,
        origin,
        installation,
        OwnedRecordingSpec {
            playlist_id: "owned-registration-member",
            snapshot: "owned-registration-member-v1",
            occurrence: "owned-registration-member-appearance",
            recording_id: "owned-registration-member-recording",
            title: "Owned distinct whole credit",
            artist: "Owned registration anchor feat. Owned registration guest",
            artists: json!([
                {"name": "owned registration anchor", "role": "main"},
                {"name": "Owned registration guest", "role": "featured"}
            ]),
        },
    )
    .await;
    let metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &registration_recording, Default::default())
        .await
        .unwrap();
    let main = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "owned registration anchor")
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    let guest = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Owned registration guest")
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    (
        anchor_recording,
        anchor_id,
        registration_recording,
        main,
        guest,
    )
}

async fn import_owned_recording(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
    spec: OwnedRecordingSpec<'_>,
) -> String {
    origin::playlist(
        origin,
        spec.playlist_id,
        spec.snapshot,
        vec![json!({
            "occurrence_id": spec.occurrence,
            "item": {"kind": "recording", "recording": {
                "id": spec.recording_id, "title": spec.title, "artist": spec.artist,
                "duration_ms": 100_000, "artists": spec.artists
            }}
        })],
    )
    .await;
    let imported = library
        .hito
        .collection
        .import_provider_playlist(
            &library.actor,
            &installation.id,
            hito_core::ImportProviderPlaylist {
                request_id: uuid::Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: format!("https://provider.invalid/{}", spec.playlist_id),
            },
        )
        .await
        .unwrap();
    let playlist = library
        .hito
        .collection
        .playlist(&library.actor, &imported.playlist_id.to_string())
        .await
        .unwrap();
    playlist.entries[0]
        .recording_id
        .as_deref()
        .unwrap()
        .to_owned()
}

async fn import_collaboration(
    library: &Library,
    origin: &origin::ProviderServer,
    installation: &hito_core::PluginInstallation,
) -> (String, String) {
    let recording = import_owned_recording(
        library,
        origin,
        installation,
        OwnedRecordingSpec {
            playlist_id: "owned-override-credit",
            snapshot: "owned-override-credit-v1",
            occurrence: "owned-override-appearance",
            recording_id: "owned-override-recording",
            title: "Owned override duet",
            artist: "Ada feat. Cy",
            artists: json!([
                {"name": "Ada", "role": "main", "join": " feat. "},
                {"name": "Cy", "role": "featured"}
            ]),
        },
    )
    .await;
    let metadata = library
        .hito
        .catalog
        .recording_metadata(&library.actor, &recording, Default::default())
        .await
        .unwrap();
    let guest = metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Cy" && member.role == hito_plugin_sdk::CreditRole::Featured)
        .unwrap()
        .artist_id
        .clone()
        .unwrap();
    (recording, guest)
}
