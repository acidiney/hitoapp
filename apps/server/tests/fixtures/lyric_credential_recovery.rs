use crate::{account, lyric_identity_fixture as library};
use axum::http::StatusCode;
use hito_plugin_sdk::{Capability, HEALTH_PATH, HealthReport, Manifest, PROTOCOL_VERSION};
use hito_testkit::fake_plugin::{FakePlugin, FakePluginBuilder, Reply};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) const TOKEN: &str = "owned-protected-recovery-token";

pub(super) fn protected_plugin(id: &str, name: &str) -> FakePluginBuilder {
    FakePlugin::builder(Manifest {
        id: id.into(),
        name: name.into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Search, Capability::Metadata, Capability::Lyrics].into(),
        authorization: None,
        content_policy: None,
    })
    .require_protocol()
    .get(HEALTH_PATH, |_| {
        Reply::Json(serde_json::to_value(HealthReport::ok()).unwrap())
    })
}

pub(super) fn reviewed_plugin(id: &str, name: &str) -> FakePluginBuilder {
    protected_plugin(id, name)
        .require_json()
        .post(hito_plugin_sdk::METADATA_PATH, |request| {
            assert_document_owner(request);
            assert_eq!(
                request.json(),
                json!({"recording_id":"owned:document-recovery"})
            );
            Reply::Json(serde_json::to_value(document_recovery_metadata()).unwrap())
        })
}

pub(super) struct SearchSetup {
    pub(super) library: library::Library,
    pub(super) installation: Value,
    peer: account::Account,
    peer_before: Value,
    before: Value,
    identities_path: String,
    identities: Value,
}

impl SearchSetup {
    pub(super) async fn prepare(plugin: &FakePlugin) -> Self {
        let library = library::Library::prepare().await;
        let installation = library
            .account
            .connect(
                &library.app,
                &plugin.url,
                "Owned credential recovery service",
                Some(TOKEN),
            )
            .await;
        let settings = library.account.get(&library.app, "/api/v1/plugins").await;
        assert_eq!(settings["installations"][0]["health"]["state"], "ok");
        let service = settings["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["id"] == installation["service_id"])
            .unwrap();
        let (status, _) = library.account.request(
            &library.app, "POST", "/api/v1/users",
            json!({"username":"lyric-recovery-peer","password":"owned credential recovery peer password"}),
        ).await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = account::Account::login(
            &library.app,
            "lyric-recovery-peer",
            "owned credential recovery peer password",
        )
        .await;
        let (status, peer_installation) = peer.request(
            &library.app, "POST", "/api/v1/plugins",
            json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-recovery-peer-token"}),
        ).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_ne!(peer_installation["id"], installation["id"]);
        let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
        assert_eq!(peer_before["installations"].as_array().unwrap().len(), 1);
        assert_eq!(
            peer_before["installations"][0]["id"],
            peer_installation["id"]
        );
        assert_eq!(peer_before["installations"][0]["health"]["state"], "ok");
        let before = library.snapshot().await;
        let identities_path = format!("/api/v1/recordings/{}/lyrics/identities", library.recording);
        let identities = library.account.get(&library.app, &identities_path).await;
        Self {
            library,
            installation,
            peer,
            peer_before,
            before,
            identities_path,
            identities,
        }
    }

    pub(super) async fn assert_preserved(&self, recovered: &Value) -> Value {
        let library = &self.library;
        let installation = &self.installation;
        let peer = &self.peer;
        let peer_before = self.peer_before.clone();
        let before = self.before.clone();
        let identities_path = &self.identities_path;
        let identities = self.identities.clone();
        let owner_installations = recovered["installations"].as_array().unwrap();
        assert_eq!(owner_installations.len(), 1);
        assert_eq!(owner_installations[0]["id"], installation["id"]);
        assert_eq!(owner_installations[0]["revision"], installation["revision"]);
        assert_eq!(owner_installations[0]["credential_set"], true);
        assert_eq!(recovered["services"], before["plugins"]["services"]);
        let (status, headers, peer_after) = account::exchange(
            &library.app,
            "GET",
            "/api/v1/plugins",
            Value::Null,
            peer.headers(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers["cache-control"], "private, no-store");
        assert_eq!(peer_after, peer_before);
        let mut after = library.snapshot().await;
        after["plugins"]["installations"][0]["health"] =
            before["plugins"]["installations"][0]["health"].clone();
        assert_eq!(after, before);
        assert_eq!(
            library.account.get(&library.app, identities_path).await,
            identities
        );
        library.assert_original();
        owner_installations[0].clone()
    }
}

pub(super) struct ReviewedSetup {
    pub(super) library: library::Library,
    pub(super) installation: Value,
    pub(super) base: String,
    pub(super) candidate: Value,
}

impl ReviewedSetup {
    pub(super) async fn prepare(plugin: &FakePlugin) -> Self {
        let library = library::Library::prepare().await;
        let installation = library
            .account
            .connect(
                &library.app,
                &plugin.url,
                "Owned document recovery service",
                Some(TOKEN),
            )
            .await;
        let base = format!("/api/v1/recordings/{}/lyrics", library.recording);
        let candidate = json!({
            "installation_id":installation["id"],
            "installation_revision":installation["revision"],
            "provider_recording_id":document_recovery_reference().id,
        });
        Self {
            library,
            installation,
            base,
            candidate,
        }
    }
}

pub(super) struct ReviewedState {
    peer: account::Account,
    peer_before: Value,
    playlist: Uuid,
}

pub(super) struct ReviewedBaseline {
    before: Value,
    preserved: Value,
}

impl ReviewedState {
    pub(super) async fn prepare(library: &library::Library, installation: &Value) -> Self {
        let (status, _) = library.account.request(
            &library.app, "POST", "/api/v1/users",
            json!({"username":"document-recovery-peer","password":"owned document recovery peer password"}),
        ).await;
        assert_eq!(status, StatusCode::CREATED);
        let peer = account::Account::login(
            &library.app,
            "document-recovery-peer",
            "owned document recovery peer password",
        )
        .await;
        let libraries = library.account.get(&library.app, "/api/v1/libraries").await;
        let shared = libraries
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["personal"] == false)
            .unwrap();
        let (status, _) = library
            .account
            .request(
                &library.app,
                "PUT",
                &format!(
                    "/api/v1/libraries/{}/members/{}",
                    shared["id"].as_str().unwrap(),
                    peer.id
                ),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let settings = library.account.get(&library.app, "/api/v1/plugins").await;
        assert_eq!(settings["installations"][0]["health"]["state"], "ok");
        let service = settings["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|service| service["id"] == installation["service_id"])
            .unwrap();
        let (status, peer_installation) = peer.request(
            &library.app, "POST", "/api/v1/plugins",
            json!({"service_id":service["id"],"service_revision":service["revision"],"bearer_token":"owned-document-peer-token"}),
        ).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_ne!(peer_installation["id"], installation["id"]);
        let peer_before = peer.get(&library.app, "/api/v1/plugins").await;
        assert_eq!(peer_before["installations"][0]["health"]["state"], "ok");
        let playlist = Uuid::new_v4();
        let operations = json!([
            {"id":Uuid::new_v4(),"action":{"kind":"recording_title_set","recording_id":library.recording,"title":"Owned document recovery title pin"}},
            {"id":Uuid::new_v4(),"action":{"kind":"like_set","recording_id":library.recording,"liked":true}},
            {"id":Uuid::new_v4(),"action":{"kind":"progress_set","recording_id":library.recording,"position_ms":700,"speed":1.25}},
            {"id":Uuid::new_v4(),"action":{"kind":"playlist_create","playlist_id":playlist,"name":"Owned document recovery playlist"}},
            {"id":Uuid::new_v4(),"action":{"kind":"entry_insert","playlist_id":playlist,"entry_id":Uuid::new_v4(),"recording_id":library.recording,"position":0}}
        ]);
        let (status, receipt) = library
            .account
            .request(
                &library.app,
                "POST",
                "/api/v1/sync",
                json!({"device_id":"owned-document-recovery-client","operations":operations}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 5);
        Self {
            peer,
            peer_before,
            playlist,
        }
    }

    pub(super) async fn capture(
        &self,
        library: &library::Library,
        base: &str,
        identity: &Value,
    ) -> ReviewedBaseline {
        let peer = &self.peer;
        let playlist = self.playlist;
        let before = library.snapshot().await;
        let preserved = document_recovery_preserved(library, peer, playlist, base).await;
        assert_eq!(preserved["owner_identities"]["items"], json!([identity]));
        assert_eq!(preserved["peer_identities"]["items"], json!([]));
        assert_eq!(
            before["metadata"]["title"]["personal"],
            "Owned document recovery title pin"
        );
        ReviewedBaseline { before, preserved }
    }

    pub(super) async fn assert_preserved(
        &self,
        library: &library::Library,
        installation: &Value,
        base: &str,
        baseline: &ReviewedBaseline,
        recovered: &Value,
    ) -> Value {
        let peer = &self.peer;
        let peer_before = self.peer_before.clone();
        let playlist = self.playlist;
        let before = baseline.before.clone();
        let preserved = baseline.preserved.clone();
        assert_eq!(recovered["installations"].as_array().unwrap().len(), 1);
        let current = &recovered["installations"][0];
        assert_eq!(current["id"], installation["id"]);
        assert_eq!(current["revision"], installation["revision"]);
        assert_eq!(current["credential_set"], true);
        assert_eq!(recovered["services"], before["plugins"]["services"]);
        assert_eq!(peer.get(&library.app, "/api/v1/plugins").await, peer_before);
        assert_eq!(
            document_recovery_preserved(library, peer, playlist, base).await,
            preserved
        );
        let mut after = library.snapshot().await;
        after["plugins"]["installations"][0]["health"] =
            before["plugins"]["installations"][0]["health"].clone();
        assert_eq!(after, before);
        library.assert_original();
        current.clone()
    }
}

pub(super) fn assert_document_owner(request: &hito_testkit::fake_plugin::RecordedRequest) {
    assert!(
        request
            .headers
            .get("authorization")
            .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(request.protocol().as_deref(), Some("1"));
    assert!(request.query.is_none());
}

pub(super) fn document_recovery_reference() -> hito_plugin_sdk::ProviderRecordingRef {
    hito_plugin_sdk::ProviderRecordingRef {
        id: "owned:document-recovery".into(),
        revision: Some("owned:document-recovery:1".into()),
    }
}

pub(super) fn document_recovery_metadata() -> hito_plugin_sdk::RecordingMetadata {
    hito_plugin_sdk::RecordingMetadata {
        recording: hito_plugin_sdk::ProviderRecording {
            id: document_recovery_reference().id,
            revision: document_recovery_reference().revision,
            title: "Owned reviewed document title".into(),
            artist: "Owned reviewed document artist".into(),
            duration_ms: None,
            release: None,
            disc_number: None,
            track_number: None,
            attribution: None,
            isrc: None,
            artist_urls: Vec::new(),
            artists: Vec::new(),
        },
        musical_title: None,
        album_title: None,
        source_ids: None,
    }
}

pub(super) fn document_recovery_response() -> hito_plugin_sdk::LyricsResponseV2 {
    hito_plugin_sdk::LyricsResponseV2 {
        recording: document_recovery_reference(),
        lyrics: Some(hito_plugin_sdk::LyricsDocument {
            id: "owned-recovery-document".into(),
            rendition_id: "owned-recovery-rendition".into(),
            revision: Some("1".into()),
            rendition_label: None,
            language: Some("pt".into()),
            duration_ms: None,
            attribution: None,
            content: hito_plugin_sdk::LyricContent::Plain {
                lines: vec!["Owned document recovery line".into()],
            },
        }),
    }
}

async fn document_recovery_preserved(
    library: &library::Library,
    peer: &account::Account,
    playlist: uuid::Uuid,
    base: &str,
) -> Value {
    let choice = format!("/api/v1/recordings/{}/source-choice", library.recording);
    let (status, progress) = library
        .account
        .request(
            &library.app,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids":[library.recording]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (owner_status, owner_choice) = library
        .account
        .request(&library.app, "GET", &choice, Value::Null)
        .await;
    let (peer_status, peer_choice) = peer
        .request(&library.app, "GET", &choice, Value::Null)
        .await;
    for (status, response) in [(owner_status, &owner_choice), (peer_status, &peer_choice)] {
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(response.get("plugin_connection").is_none());
        let encoded = response.to_string();
        for private in [
            TOKEN,
            "owned-document-peer-token",
            "owned:document-recovery",
            "Owned reviewed document title",
            "Owned reviewed document artist",
        ] {
            assert!(!encoded.contains(private));
        }
    }
    json!({
        "owner_identities":library.account.get(&library.app, &format!("{base}/identities")).await,
        "peer_identities":peer.get(&library.app, &format!("{base}/identities")).await,
        "owner_choice":{"status":owner_status.as_u16(),"error":owner_choice},
        "peer_choice":{"status":peer_status.as_u16(),"error":peer_choice},
        "liked":library.account.get(&library.app, "/api/v1/recordings?liked=true").await,
        "playlist":library.account.get(&library.app, &format!("/api/v1/playlists/{playlist}")).await,
        "progress":progress,
    })
}

pub(super) async fn private_settings(library: &library::Library) -> Value {
    let (status, headers, recovered) = account::exchange(
        &library.app,
        "GET",
        "/api/v1/plugins",
        Value::Null,
        library.account.headers(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    recovered
}

pub(super) fn empty_search_page() -> hito_plugin_sdk::SearchPage {
    hito_plugin_sdk::SearchPage {
        items: Vec::new(),
        next_cursor: None,
    }
}

pub(super) fn assert_search_owner(request: &hito_testkit::fake_plugin::RecordedRequest) {
    assert!(
        request
            .headers
            .get("authorization")
            .is_some_and(|value| value == format!("Bearer {TOKEN}").as_str())
    );
}

impl ReviewedBaseline {
    pub(super) fn after_public_title_edit(
        &self,
        recording: &str,
        operation: Uuid,
        device: &str,
        title: &str,
        receipt: &Value,
        history: &Value,
    ) -> Self {
        assert_eq!(receipt, &json!({"acknowledged":[operation]}));
        let previous_events = self.before["events"]["operations"].as_array().unwrap();
        assert_eq!(self.before["events"]["has_more"], false);
        let events = history["operations"].as_array().unwrap();
        assert_eq!(history.as_object().unwrap().len(), 3);
        assert_eq!(history["has_more"], false);
        assert_eq!(events.len(), previous_events.len() + 1);
        assert_eq!(&events[..previous_events.len()], previous_events.as_slice());
        let accepted = &events[previous_events.len()];
        assert_eq!(accepted.as_object().unwrap().len(), 3);
        assert_eq!(accepted["device_id"], device);
        assert_eq!(
            accepted["operation"],
            json!({
                "id":operation,
                "action":{"kind":"recording_title_set","recording_id":recording,"title":title},
            })
        );
        assert!(
            accepted["sequence"].as_i64().unwrap()
                > self.before["events"]["cursor"].as_i64().unwrap()
        );
        assert_eq!(history["cursor"], accepted["sequence"]);
        let old_title = &self.before["metadata"]["title"]["value"];
        assert_ne!(old_title, &json!(title));
        let replace_title = |items: &mut Value, key: &str| {
            let mut replaced = 0;
            for row in items.as_array_mut().unwrap() {
                if row[key] == recording {
                    assert_eq!(&row["title"], old_title);
                    row["title"] = json!(title);
                    replaced += 1;
                }
            }
            assert_eq!(replaced, 1);
        };
        let mut before = self.before.clone();
        let mut preserved = self.preserved.clone();
        replace_title(&mut before["catalog"]["items"], "id");
        replace_title(&mut before["activity"]["history"], "id");
        replace_title(&mut preserved["liked"]["items"], "id");
        replace_title(&mut preserved["playlist"]["entries"], "recording_id");
        before["metadata"]["title"]["value"] = json!(title);
        before["metadata"]["title"]["personal"] = json!(title);
        before["metadata"]["title"]["operation_id"] = json!(operation);
        before["events"] = history.clone();
        Self { before, preserved }
    }
}
