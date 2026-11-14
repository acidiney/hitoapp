use axum::http::{HeaderMap, StatusCode};
use hito_audio::test_support::wave;
use hito_core::test_support::{database_owner::DatabaseOwner, hang_guard::hang_guard, support};
use hito_core::{
    Action, Actor, AdoptProviderRecording, CoreConfig, Credentials, HostDeviceId, InstallPlugin,
    OfferPluginService, PluginInstallation, Services, SourcePreferenceEntry,
    UpdateSourcePreferences,
};
use hito_plugin_sdk::{
    Manifest, ProviderRecording, RecordingRequest, SearchPage, SearchRequest, SourcePage,
};
use hito_testkit::fake_plugin::{FakePlugin, RecordedRequest, Reply};
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

pub const TITLE: &str = "Owned retry tune";
pub const ARTIST: &str = "Owned retry performer";
pub const PRIVATE_TITLE: &str = "private-164-title-never-log";
pub const TOKEN: &str = "private-164-token-never-log";
const PRIMARY_ID: &str = "owned-primary";
const CANDIDATE_ID: &str = "owned-candidate";
const SOURCE_ID: &str = "owned-ready-audio";

pub struct Fixture {
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    pub recording: Uuid,
    pub sentinel: Uuid,
    pub first: FakePlugin,
    pub second: FakePlugin,
    pub first_installation: PluginInstallation,
    pub second_installation: PluginInstallation,
    pub order: Arc<Mutex<Vec<char>>>,
    pub invalid: Arc<AtomicBool>,
    pub directory: Arc<tempfile::TempDir>,
    primary: FakePlugin,
    database: DatabaseOwner,
    original: Vec<u8>,
    sentinel_original: Vec<u8>,
    metadata_before: Value,
}

impl Fixture {
    pub async fn start_with_ready_on_attempt(ready_on_attempt: Option<usize>) -> Self {
        hang_guard(
            "owned occurrence retry setup",
            Box::pin(async {
                let directory = Arc::new(tempfile::tempdir().unwrap());
                let music = directory.path().join("music");
                std::fs::create_dir(&music).unwrap();
                let database = DatabaseOwner::retain(directory.clone());
                let mut config = CoreConfig::new(
                    directory.path().join("hito.db"),
                    music.clone(),
                    HostDeviceId::new("native-occurrence-retry").unwrap(),
                );
                database.attach(&mut config);
                let services = Box::pin(hito_core::open(config)).await.unwrap();
                let session = services
                    .identity
                    .setup(Credentials {
                        username: "admin".into(),
                        password: "owned occurrence retry password".into(),
                    })
                    .await
                    .unwrap();
                let actor = services
                    .identity
                    .authenticate(&session.token)
                    .await
                    .unwrap();
                let original = owned_wave(440.);
                let sentinel_original = owned_wave(880.);
                std::fs::write(directory.path().join("owned-provider.wav"), &original).unwrap();
                std::fs::write(music.join("sentinel.wav"), &sentinel_original).unwrap();
                let library = services
                    .catalog
                    .libraries(&actor)
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|library| !library.personal)
                    .unwrap()
                    .id;
                support::scan(&services, &actor, &library).await;
                let sentinel = services
                    .catalog
                    .recordings(&actor, Default::default())
                    .await
                    .unwrap()
                    .items
                    .into_iter()
                    .find(|recording| recording.library_id == library)
                    .unwrap();
                let invalid = Arc::new(AtomicBool::new(false));
                let rejected = invalid.clone();
                let primary = FakePlugin::builder(manifest("primary", &["metadata"]))
                    .require_protocol()
                    .require_json()
                    .strict(16)
                    .post("v1/recordings/detail", move |request| {
                        if !recording_request(request, PRIMARY_ID) {
                            rejected.store(true, Ordering::SeqCst);
                            return Reply::Status(StatusCode::BAD_REQUEST);
                        }
                        Reply::Json(recording(PRIMARY_ID))
                    })
                    .start()
                    .await;
                let primary_installation = connect(&services, &actor, &primary).await;
                let target = services
                    .catalog
                    .adopt_provider(
                        &actor,
                        &primary_installation.id,
                        AdoptProviderRecording {
                            installation_revision: primary_installation.revision,
                            provider_recording_id: PRIMARY_ID.into(),
                        },
                    )
                    .await
                    .unwrap();
                let recording = Uuid::parse_str(&target.id).unwrap();
                let sentinel = Uuid::parse_str(&sentinel.id).unwrap();
                support::synchronize(
                    &services,
                    &actor,
                    vec![
                        Action::LikeSet {
                            recording_id: recording,
                            liked: true,
                        },
                        Action::RecordingTitleSet {
                            recording_id: recording,
                            title: PRIVATE_TITLE.into(),
                        },
                        Action::ProgressSet {
                            recording_id: sentinel,
                            position_ms: 400,
                            speed: 1.25,
                        },
                    ],
                )
                .await;
                assert!(
                    services
                        .catalog
                        .sources(&actor, &target.id)
                        .await
                        .unwrap()
                        .is_empty(),
                    "the valid owned recording must begin without audio"
                );
                let order = Arc::new(Mutex::new(Vec::new()));
                let first = discovery('A', None, &order, &invalid, &original).await;
                let second = discovery('B', ready_on_attempt, &order, &invalid, &original).await;
                let first_installation = connect(&services, &actor, &first).await;
                let second_installation = connect(&services, &actor, &second).await;
                let metadata_before = preserved_metadata(&services, &actor, recording).await;
                let fixture = Self {
                    services,
                    actor,
                    token: session.token,
                    recording,
                    sentinel,
                    first,
                    second,
                    first_installation,
                    second_installation,
                    order,
                    invalid,
                    directory,
                    primary,
                    database,
                    original,
                    sentinel_original,
                    metadata_before,
                };
                fixture.rank(false).await;
                fixture
            }),
        )
        .await
    }

    pub async fn rank(&self, reversed: bool) {
        let current = self
            .services
            .catalog
            .source_preferences(&self.actor)
            .await
            .unwrap();
        let ids = if reversed {
            [&self.second_installation.id, &self.first_installation.id]
        } else {
            [&self.first_installation.id, &self.second_installation.id]
        };
        self.services
            .catalog
            .update_source_preferences(
                &self.actor,
                UpdateSourcePreferences {
                    revision: current.revision,
                    fallback: true,
                    entries: ids
                        .into_iter()
                        .map(|id| SourcePreferenceEntry::Provider {
                            installation_id: id.clone(),
                        })
                        .collect(),
                },
            )
            .await
            .unwrap();
    }

    pub async fn preserved(&self) -> bool {
        let metadata = preserved_metadata(&self.services, &self.actor, self.recording).await;
        let target = self
            .services
            .catalog
            .recordings_by_ids(&self.actor, &[self.recording])
            .await
            .unwrap();
        let sentinel = self
            .services
            .collection
            .saved_position(&self.actor, self.sentinel)
            .await
            .unwrap();
        metadata == self.metadata_before
            && target.len() == 1
            && target[0].liked
            && sentinel.is_some_and(|position| position.position_ms == 400)
            && std::fs::read(self.directory.path().join("owned-provider.wav")).unwrap()
                == self.original
            && std::fs::read(self.directory.path().join("music/sentinel.wav")).unwrap()
                == self.sentinel_original
    }

    pub async fn preserved_target_and_files(&self) -> bool {
        let metadata = preserved_metadata(&self.services, &self.actor, self.recording).await;
        let target = self
            .services
            .catalog
            .recordings_by_ids(&self.actor, &[self.recording])
            .await
            .unwrap();
        metadata == self.metadata_before
            && target.len() == 1
            && target[0].liked
            && std::fs::read(self.directory.path().join("owned-provider.wav")).unwrap()
                == self.original
            && std::fs::read(self.directory.path().join("music/sentinel.wav")).unwrap()
                == self.sentinel_original
    }

    pub fn protocol_clean(&self) -> bool {
        !self.invalid.load(Ordering::SeqCst)
            && [&self.primary, &self.first, &self.second]
                .into_iter()
                .all(|provider| {
                    provider.unexpected() == 0
                        && provider.requests().len() <= 32
                        && provider.requests().iter().all(|request| {
                            request.handled == hito_testkit::fake_plugin::Handled::Yes
                        })
                })
    }

    pub fn private_values(&self) -> Vec<String> {
        vec![
            self.token.clone(),
            self.primary.url.clone(),
            self.first.url.clone(),
            self.second.url.clone(),
            self.directory.path().to_string_lossy().into_owned(),
        ]
    }

    pub async fn close(mut self) -> bool {
        let playback = self.services.playback.close().await.is_ok();
        let replicas = self.services.replicas.close().await.is_ok();
        let catalog = self.services.catalog_replication.close().await.is_ok();
        let first = self.first.finish().await;
        let second = self.second.finish().await;
        let primary = self.primary.finish().await;
        drop(self.services);
        let retired = self.database.retire(Duration::from_secs(3)).await;
        playback
            && replicas
            && catalog
            && first
            && second
            && primary
            && retired.resources_retired
            && !retired.deadline_exceeded
    }
}

async fn discovery(
    label: char,
    ready_on_attempt: Option<usize>,
    order: &Arc<Mutex<Vec<char>>>,
    invalid: &Arc<AtomicBool>,
    original: &[u8],
) -> FakePlugin {
    let detail = recording(CANDIDATE_ID);
    let sources = json!({"items":[{"id":SOURCE_ID,"label":"Owned audio","ready":true}]});
    assert!(
        serde_json::from_value::<ProviderRecording>(detail.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    assert!(
        serde_json::from_value::<SourcePage>(sources.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    let ready = json!({"items":[detail.clone()],"next_cursor":null});
    let empty = json!({"items":[],"next_cursor":null});
    for page in [&ready, &empty] {
        assert!(
            serde_json::from_value::<SearchPage>(page.clone())
                .unwrap()
                .validate(20)
                .is_ok()
        );
    }
    let searches = AtomicUsize::new(0);
    let ordering = order.clone();
    let rejected_search = invalid.clone();
    let rejected_detail = invalid.clone();
    let rejected_sources = invalid.clone();
    let rejected_audio = invalid.clone();
    let audio = original.to_vec();
    FakePlugin::builder(manifest(&label.to_string(), &["search", "sources"]))
        .require_protocol()
        .require_json()
        .post("v1/search", move |request| {
            let valid =
                serde_json::from_slice::<SearchRequest>(&request.body).is_ok_and(|request| {
                    request.validate().is_ok()
                        && request.query == format!("{ARTIST} {TITLE}")
                        && request.cursor.is_none()
                        && request.limit == 20
                });
            if !valid {
                rejected_search.store(true, Ordering::SeqCst);
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            ordering.lock().unwrap().push(label);
            let attempt = searches.fetch_add(1, Ordering::SeqCst) + 1;
            Reply::Json(if ready_on_attempt == Some(attempt) {
                ready.clone()
            } else {
                empty.clone()
            })
        })
        .post("v1/recordings/detail", move |request| {
            if ready_on_attempt.is_none() || !recording_request(request, CANDIDATE_ID) {
                rejected_detail.store(true, Ordering::SeqCst);
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            Reply::Json(detail.clone())
        })
        .post("v1/sources", move |request| {
            if ready_on_attempt.is_none() || !recording_request(request, CANDIDATE_ID) {
                rejected_sources.store(true, Ordering::SeqCst);
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            Reply::Json(sources.clone())
        })
        .get("v1/audio", move |request| {
            if ready_on_attempt.is_none()
                || request.query.as_deref() != Some("source_id=owned-ready-audio")
            {
                rejected_audio.store(true, Ordering::SeqCst);
                return Reply::Status(StatusCode::BAD_REQUEST);
            }
            Reply::Raw {
                status: StatusCode::OK,
                headers: HeaderMap::from_iter([(
                    "content-type".parse().unwrap(),
                    "audio/wav".parse().unwrap(),
                )]),
                body: audio.clone(),
                chunked: false,
            }
        })
        .start()
        .await
}

fn recording_request(request: &RecordedRequest, expected: &str) -> bool {
    serde_json::from_slice::<RecordingRequest>(&request.body)
        .is_ok_and(|request| request.validate().is_ok() && request.recording_id == expected)
}

fn recording(id: &str) -> Value {
    json!({"id":id,"title":TITLE,"artist":ARTIST,"duration_ms":8000,"revision":"owned-r1"})
}

fn manifest(suffix: &str, capabilities: &[&str]) -> Manifest {
    serde_json::from_value(json!({
        "id":format!("org.hito.occurrence-{suffix}"),"name":"Owned occurrence fixture",
        "protocol_version":1,"capabilities":capabilities,
    }))
    .unwrap()
}

async fn connect(services: &Services, actor: &Actor, provider: &FakePlugin) -> PluginInstallation {
    let service = services
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned occurrence fixture".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = services
        .plugins
        .install(
            actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: Some(TOKEN.into()),
                accept_manifest_digest: None,
            },
        )
        .await
        .unwrap();
    provider.require_bearer(TOKEN);
    installation
}

fn owned_wave(frequency: f64) -> Vec<u8> {
    let mut wave = wave(48_000, 1, 48_000 * 8, |frame, _| {
        ((f64::from(frame) * frequency * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
    });
    let mut bytes = Vec::new();
    wave.read_to_end(&mut bytes).unwrap();
    bytes
}

async fn preserved_metadata(services: &Services, actor: &Actor, recording: Uuid) -> Value {
    let metadata = services
        .catalog
        .recording_metadata(actor, &recording.to_string(), Default::default())
        .await
        .unwrap();
    json!({"title":metadata.title,"artist":metadata.artist,"selected":metadata.selected_observation})
}
