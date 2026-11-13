use crate::{
    account::{Account, exchange},
    artist_review_fixture::origin_counts,
    credit_member_review_credentials::private_snapshot,
    credit_member_review_preview_fixture::{Context, Library, private},
    provider_fixture::ProviderServer,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use hito_core::{
    Action, ArtistReviewContext, CreditMemberReviewContext, Operation,
    test_support::{artist_catalog_discovery_service as catalog, playlist_service},
};
use hito_plugin_sdk::{CreditRole, ProviderRecording};
use serde_json::{Value, json};
use uuid::Uuid;

const MAIN: &str = "Owned reset Main qualification";
const SECOND_MAIN: &str = "Owned reset second Main";
const FEATURED: &str = "Owned reset Featured";
const TITLE: &str = "Private reset adoption title";
pub(crate) type Response = (StatusCode, HeaderMap, Value);

pub(crate) struct Scenario {
    pub(crate) recording: String,
    pub(crate) whole: ArtistReviewContext,
    pub(crate) main: CreditMemberReviewContext,
    pub(crate) featured: CreditMemberReviewContext,
    playlists: Vec<String>,
    original_recording: String,
    original_playlist: String,
}

impl Scenario {
    pub(crate) async fn prepare(owned: &Context, origin: &ProviderServer) -> Self {
        let original = owned.snapshot().await;
        let original_recording = owned.context["recording_id"].as_str().unwrap().to_owned();
        let original_playlist = original["library"]["playlist"]["playlist"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        origin
            .reply(
                "v1/manifest",
                json!({
                    "id": "org.hito.reset-main-adoption", "name": "Owned reset adoption metadata",
                    "protocol_version": 1, "capabilities": ["playlists"],
                }),
            )
            .await;
        let service = expect_status(
            &owned.app,
            &owned.account,
            "POST",
            "/api/v1/plugins/services",
            json!({"name": "Owned reset adoption metadata", "base_url": origin.url}),
            StatusCode::CREATED,
        )
        .await;
        let installation = expect_status(
            &owned.app,
            &owned.account,
            "POST",
            "/api/v1/plugins",
            json!({"service_id": service["id"], "service_revision": service["revision"],
                "bearer_token": null}),
            StatusCode::CREATED,
        )
        .await;
        let (text_playlist, recording) = import_snapshot(owned, origin, &installation, 0).await;
        let whole_path = format!("/api/v1/recordings/{recording}/artist-review");
        let provider = owned.context["provider"].clone();
        let text: ArtistReviewContext = serde_json::from_value(
            accepted(
                &owned.app,
                &owned.account,
                "POST",
                &format!("{whole_path}/context"),
                json!({"provider": provider}),
            )
            .await,
        )
        .unwrap();
        assert_eq!(text.credited_name, MAIN);
        let recorded = Uuid::parse_str(&recording).unwrap();
        let operations = [
            Action::RecordingTitleSet {
                recording_id: recorded,
                title: TITLE.into(),
            },
            Action::LikeSet {
                recording_id: recorded,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recorded,
                position_ms: 400,
                speed: 1.25,
            },
        ]
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect::<Vec<_>>();
        let synced = accepted(
            &owned.app,
            &owned.account,
            "POST",
            "/api/v1/sync",
            json!({
                "device_id": "member-reset-adoption-http", "operations": operations,
            }),
        )
        .await;
        assert_eq!(
            synced["acknowledged"],
            json!(operations.iter().map(|op| op.id).collect::<Vec<_>>())
        );
        let (main_playlist, main_recording) =
            import_snapshot(owned, origin, &installation, 1).await;
        assert_eq!(main_recording, recording);
        let adopted: ArtistReviewContext = serde_json::from_value(
            accepted(
                &owned.app,
                &owned.account,
                "POST",
                &format!("{whole_path}/context"),
                json!({"provider": provider}),
            )
            .await,
        )
        .unwrap();
        assert_eq!(adopted.artist_id, text.artist_id);
        assert_ne!(adopted.observation_id, text.observation_id);
        let (featured_playlist, featured_recording) =
            import_snapshot(owned, origin, &installation, 2).await;
        assert_eq!(featured_recording, recording);
        assert_ne!(text_playlist, main_playlist);
        assert_ne!(main_playlist, featured_playlist);
        assert_ne!(text_playlist, featured_playlist);
        let metadata = accepted(
            &owned.app,
            &owned.account,
            "GET",
            &format!("/api/v1/recordings/{recording}/metadata"),
            Value::Null,
        )
        .await;
        assert_eq!(metadata["title"]["personal"], TITLE);
        assert!(metadata["artist"]["personal"].is_null());
        let credits = metadata["credited_artists"].as_array().unwrap();
        assert_eq!(credits.len(), 3);
        for (credit, name, role) in [
            (&credits[0], MAIN, "main"),
            (&credits[1], SECOND_MAIN, "main"),
            (&credits[2], FEATURED, "featured"),
        ] {
            assert_eq!(credit["name"], name);
            assert_eq!(credit["role"], role);
            assert!(
                credit["artist_id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty())
            );
        }
        let whole: ArtistReviewContext = serde_json::from_value(
            accepted(
                &owned.app,
                &owned.account,
                "POST",
                &format!("{whole_path}/context"),
                json!({"provider": provider}),
            )
            .await,
        )
        .unwrap();
        assert_eq!(whole.artist_id, text.artist_id);
        assert_eq!(credits[0]["artist_id"], whole.artist_id);
        assert_ne!(whole.observation_id, adopted.observation_id);
        assert_eq!(metadata["selected_observation"]["id"], whole.observation_id);
        let association = accepted(
            &owned.app,
            &owned.account,
            "POST",
            &format!("{whole_path}/confirm"),
            json!({"context": whole, "provider_artist_id": catalog::FIRST_ARTIST}),
        )
        .await;
        let chosen = accepted(
            &owned.app,
            &owned.account,
            "PUT",
            &format!("/api/v1/recordings/{recording}/artist-identity"),
            json!({
                "request_id": Uuid::new_v4(), "expected_revision": 0,
                "association_id": association["id"], "context": whole,
            }),
        )
        .await;
        assert_eq!(chosen["revision"], 1);
        assert_eq!(chosen["selection"]["association_id"], association["id"]);
        let mut members = Vec::new();
        for (credit, name, role) in [
            (&credits[0], MAIN, CreditRole::Main),
            (&credits[2], FEATURED, CreditRole::Featured),
        ] {
            let artist = credit["artist_id"].as_str().unwrap();
            let context: CreditMemberReviewContext = serde_json::from_value(accepted(
                &owned.app, &owned.account, "POST",
                &format!("/api/v1/recordings/{recording}/credit-members/{artist}/artist-review/context"),
                json!({"provider": provider})).await).unwrap();
            assert_eq!(context.account_id, owned.account.id);
            assert_eq!(context.recording_id, recording);
            assert_eq!(context.library_id, whole.library_id);
            assert_eq!(context.artist_id, artist);
            assert_eq!(context.credited_name, name);
            assert_eq!(context.role, role);
            assert_eq!(context.observation_id, whole.observation_id);
            assert!(context.metadata_binding == whole.metadata_binding);
            assert!(context.connection == whole.connection);
            let choice = accepted(
                &owned.app,
                &owned.account,
                "GET",
                &format!("/api/v1/recordings/{recording}/credit-members/{artist}/artist-identity"),
                Value::Null,
            )
            .await;
            assert_eq!(
                choice,
                json!({"recording_id": recording, "artist_id": artist,
                "role": role, "revision": 0, "state": {"status": "unselected"}})
            );
            members.push(context);
        }
        let featured = members.pop().unwrap();
        let main = members.pop().unwrap();
        assert_eq!(main.artist_id, whole.artist_id);
        assert_ne!(main.artist_id, featured.artist_id);
        let progress = accepted(
            &owned.app,
            &owned.account,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [recording]}),
        )
        .await;
        assert_eq!(progress[0]["position_ms"], 400);
        assert_eq!(progress[0]["speed"], 1.25);
        let recordings = accepted(
            &owned.app,
            &owned.account,
            "GET",
            "/api/v1/recordings",
            Value::Null,
        )
        .await;
        let retained = recordings["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == recording)
            .unwrap();
        assert_eq!(retained["liked"], true);
        let sources = accepted(
            &owned.app,
            &owned.account,
            "GET",
            &format!("/api/v1/recordings/{recording}/sources"),
            Value::Null,
        )
        .await;
        assert_eq!(sources, json!([]));
        Self {
            recording,
            whole,
            main,
            featured,
            playlists: vec![text_playlist, main_playlist, featured_playlist],
            original_recording,
            original_playlist,
        }
    }

    pub(crate) async fn assert_contexts_current(&self, owned: &Context) {
        for context in [&self.main, &self.featured] {
            let current = accepted(
                &owned.app,
                &owned.account,
                "POST",
                &format!(
                    "/api/v1/recordings/{}/credit-members/{}/artist-review/context",
                    self.recording, context.artist_id
                ),
                json!({"provider": context.provider}),
            )
            .await;
            assert_eq!(current, json!(context));
        }
        let whole = accepted(
            &owned.app,
            &owned.account,
            "POST",
            &format!(
                "/api/v1/recordings/{}/artist-review/context",
                self.recording
            ),
            json!({"provider": self.whole.provider}),
        )
        .await;
        assert_eq!(whole, json!(self.whole));
    }
}

async fn import_snapshot(
    owned: &Context,
    origin: &ProviderServer,
    installation: &Value,
    stage: usize,
) -> (String, String) {
    let mut recording = json!({"id": "owned-reset-adoption-recording", "title": "Owned reset adoption recording",
        "artist": MAIN, "duration_ms": 100_000});
    if stage > 0 {
        let mut artists = vec![
            json!({"name": MAIN, "role": "main", "join": " & "}),
            json!({"name": SECOND_MAIN, "role": "main"}),
        ];
        if stage == 2 {
            artists[1]["join"] = json!(" feat. ");
            artists.push(json!({"name": FEATURED, "role": "featured"}));
        }
        recording["artists"] = json!(artists);
    }
    serde_json::from_value::<ProviderRecording>(recording.clone())
        .unwrap()
        .validate()
        .unwrap();
    playlist_service::playlist(
        origin,
        "owned-reset-adoption",
        &format!("owned-reset-adoption:{stage}"),
        vec![json!({"occurrence_id": "owned-reset-adoption-occurrence",
            "item": {"kind": "recording", "recording": recording}})],
    )
    .await;
    let receipt = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({"request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/owned-reset-adoption"}),
    )
    .await;
    let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
    let copy = accepted(
        &owned.app,
        &owned.account,
        "GET",
        &format!("/api/v1/playlists/{playlist}"),
        Value::Null,
    )
    .await;
    let entries = copy["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    (
        playlist,
        entries[0]["recording_id"].as_str().unwrap().to_owned(),
    )
}

pub(crate) async fn accepted(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
) -> Value {
    expect_status(app, account, method, path, body, StatusCode::OK).await
}

async fn expect_status(
    app: &Router,
    account: &Account,
    method: &str,
    path: &str,
    body: Value,
    expected: StatusCode,
) -> Value {
    let (status, headers, value) = exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, expected, "{method} {path} returned HTTP {status}");
    private(&headers);
    value
}

pub(crate) struct Checkpoint {
    pub(crate) state: Value,
    pub(crate) groups: Value,
    pub(crate) whole_group: Value,
    peer: Value,
    origin: Vec<usize>,
    provider_requests: usize,
    unexpected: usize,
}

impl Checkpoint {
    pub(crate) async fn capture(
        library: &Library,
        owned: &Context,
        origin: &ProviderServer,
        scenario: &Scenario,
    ) -> Self {
        let mut state = private_snapshot(
            &owned.app,
            &owned.account,
            &scenario.recording,
            &scenario.playlists[2],
        )
        .await;
        state["original"] = private_snapshot(
            &owned.app,
            &owned.account,
            &scenario.original_recording,
            &scenario.original_playlist,
        )
        .await;
        let mut playlists = Vec::new();
        for playlist in &scenario.playlists {
            playlists.push(
                accepted(
                    &owned.app,
                    &owned.account,
                    "GET",
                    &format!("/api/v1/playlists/{playlist}"),
                    Value::Null,
                )
                .await,
            );
        }
        state["adoption_playlists"] = json!(playlists);
        for (field, path) in [
            ("playlists", "/api/v1/playlists"),
            ("activity", "/api/v1/activity"),
            ("plugins", "/api/v1/plugins"),
            ("artists", "/api/v1/artists"),
            ("releases", "/api/v1/releases"),
            ("libraries", "/api/v1/libraries"),
        ] {
            state[field] = accepted(&owned.app, &owned.account, "GET", path, Value::Null).await;
        }
        let download = exchange(
            &owned.app,
            "GET",
            &format!("/api/v1/recordings/{}/download", scenario.recording),
            Value::Null,
            owned.account.headers(),
        )
        .await;
        assert_eq!(download.0, StatusCode::NOT_FOUND);
        private(&download.1);
        state["download"] = json!({"status": download.0.as_u16(), "body": download.2});
        state["changes"] = accepted(
            &owned.app,
            &owned.account,
            "GET",
            &format!(
                "/api/v1/recordings/{}/artist-association-changes",
                scenario.recording
            ),
            Value::Null,
        )
        .await;
        let group = state["choice"]["state"]["group_id"].as_str().unwrap();
        let whole_group = accepted(
            &owned.app,
            &owned.account,
            "GET",
            &format!("/api/v1/artist-identity-groups/{group}"),
            Value::Null,
        )
        .await;
        let groups = state["groups"].clone();
        let peer = json!({
            "private": owned.peer_snapshot().await,
            "policy": owned.peer.get(&owned.app, "/api/v1/metadata/settings").await,
            "events": owned.peer.get(&owned.app, "/api/v1/sync?cursor=0").await,
            "groups": owned.peer.get(&owned.app, "/api/v1/artist-identity-groups").await,
        });
        let mut origins = library.origin_counts().await;
        origins.extend(origin_counts(origin).await);
        Self {
            state,
            groups,
            whole_group,
            peer,
            origin: origins,
            provider_requests: library.provider.as_ref().unwrap().requests().len(),
            unexpected: library.provider.as_ref().unwrap().unexpected(),
        }
    }

    pub(crate) fn assert_private_preserved(&self, before: &Self) {
        assert_eq!(
            serde_json::to_vec(&self.state).unwrap(),
            serde_json::to_vec(&before.state).unwrap(),
            "Member reset must preserve complete Whole and original recording, metadata, associations, history, collection, playlists, progress, sync, plugins and policy state"
        );
        assert_eq!(self.whole_group, before.whole_group);
        assert_eq!(self.peer, before.peer);
        assert_eq!(self.origin, before.origin);
        assert_eq!(self.provider_requests, before.provider_requests);
        assert_eq!(self.unexpected, before.unexpected);
        assert_eq!(self.unexpected, 0);
    }
}

pub(crate) struct Reads {
    pub(crate) current: Response,
    pub(crate) detail: Response,
    pub(crate) filtered: Response,
    pub(crate) peer_detail: Response,
}

impl Reads {
    pub(crate) async fn capture(
        owned: &Context,
        scenario: &Scenario,
        choice_path: &str,
        group: &str,
    ) -> Self {
        let detail_path = format!("/api/v1/artist-identity-groups/{group}");
        Self {
            current: exchange(
                &owned.app,
                "GET",
                choice_path,
                Value::Null,
                owned.account.headers(),
            )
            .await,
            detail: exchange(
                &owned.app,
                "GET",
                &detail_path,
                Value::Null,
                owned.account.headers(),
            )
            .await,
            filtered: exchange(
                &owned.app,
                "GET",
                &format!(
                    "{detail_path}?library={}&recording_offset=0&limit=1",
                    scenario.whole.library_id
                ),
                Value::Null,
                owned.account.headers(),
            )
            .await,
            peer_detail: exchange(
                &owned.app,
                "GET",
                &detail_path,
                Value::Null,
                owned.peer.headers(),
            )
            .await,
        }
    }
}
