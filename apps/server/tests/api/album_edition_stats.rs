use crate::{
    album_edition_stats_fixture as catalog,
    artist_review_fixture::{self as review, Account},
    support,
};
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    Action, CoreConfig, HostDeviceId, Operation,
    test_support::{
        album_edition_service as album, database_owner::DatabaseOwner, hang_guard::hang_guard,
    },
};
use hito_testkit::fake_plugin::FakePlugin;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

struct Selected {
    account: Account,
    peer: Account,
    installation: Value,
    recordings: Vec<String>,
    release: String,
    association: String,
}

#[tokio::test]
async fn a_saved_library_edition_reports_audio_occurrences_without_changing_its_collection() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let audio = music.join("owned-library-stats-original.wav");
    support::write_wav(&audio);
    let original = std::fs::read(&audio).unwrap();
    let owner = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("library-edition-stats-http").unwrap(),
    );
    owner.attach(&mut config);
    let services = hang_guard(
        "library stats owned services",
        Box::pin(hito_core::open(config)),
    )
    .await
    .unwrap();
    let app = hito_server::router(services.clone());
    let mut provider = hang_guard("library stats owned catalog", Box::pin(catalog::start())).await;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let selected = hang_guard(
            "library stats public choice",
            Box::pin(select(&app, &provider)),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            read_saved(&app, &selected, &provider),
        )
        .await
        .expect("The saved library edition read must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    let joined = provider.finish().await;
    let playback_closed = services.playback.close().await;
    let replicas_closed = services.replicas.close().await;
    let replication_closed = services.catalog_replication.close().await;
    drop(app);
    drop(services);
    let retired = owner.retire(Duration::from_secs(5)).await;
    assert!(joined);
    assert!(playback_closed.is_ok() && replicas_closed.is_ok() && replication_closed.is_ok());
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(owner.observe().active_resources(), 0);
    assert_eq!(std::fs::read(audio).unwrap(), original);
    let response = match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        response.get("stats"),
        Some(&json!({"audio_tracks": 4, "total_duration_ms": null})),
        "A library album must report its selected edition's audio occurrences and unknown total duration"
    );
}

async fn select(app: &Router, provider: &FakePlugin) -> Selected {
    let account = Account::setup(app).await;
    let installation = account
        .connect(app, &provider.url, "Owned library statistics", None)
        .await;
    let mut recordings: Vec<String> = Vec::new();
    let mut release = String::new();
    for id in [album::FIRST, album::SECOND, album::EXTRA] {
        let recording = accepted(
            app,
            &account,
            "POST",
            &format!(
                "/api/v1/plugins/{}/recordings",
                installation["id"].as_str().unwrap()
            ),
            json!({"installation_revision": installation["revision"], "provider_recording_id": id}),
        )
        .await;
        if release.is_empty() {
            release = recording["release_id"].as_str().unwrap().into();
        }
        assert_eq!(recording["release_id"], release);
        recordings.push(recording["id"].as_str().unwrap().into());
    }
    let operations = [
        Action::RecordingTitleSet {
            recording_id: Uuid::parse_str(&recordings[1]).unwrap(),
            title: "Owned personal library title".into(),
        },
        Action::LikeSet {
            recording_id: Uuid::parse_str(&recordings[0]).unwrap(),
            liked: true,
        },
        Action::ProgressSet {
            recording_id: Uuid::parse_str(&recordings[0]).unwrap(),
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
    let accepted_operations = accepted(
        app,
        &account,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "owned-library-statistics", "operations": operations,
        }),
    )
    .await;
    assert_eq!(
        accepted_operations["acknowledged"],
        json!(
            operations
                .iter()
                .map(|operation| operation.id)
                .collect::<Vec<_>>()
        )
    );
    let settings = account.get(app, "/api/v1/metadata/settings").await;
    let selected = json!({"kind": "http_plugin", "installation_id": installation["id"]});
    accepted(
        app,
        &account,
        "PUT",
        "/api/v1/metadata/settings",
        json!({
            "revision": settings["revision"], "enabled": true, "providers": [selected],
        }),
    )
    .await;
    let review = format!("/api/v1/recordings/{}/artist-review", recordings[0]);
    let context = accepted(
        app,
        &account,
        "POST",
        &format!("{review}/context"),
        json!({"provider": selected}),
    )
    .await;
    let decision = json!({"context": context, "provider_artist_id": album::ARTIST_ID});
    let preview = accepted(
        app,
        &account,
        "POST",
        &format!("{review}/preview"),
        decision.clone(),
    )
    .await;
    assert_eq!(preview["status"], "available");
    let association = accepted(
        app,
        &account,
        "POST",
        &format!("{review}/confirm"),
        decision,
    )
    .await;
    let association = association["id"].as_str().unwrap().to_owned();
    let before_choice = provider.requests().len();
    let receipt = accepted(
        app,
        &account,
        "PUT",
        &format!("/api/v1/albums/{release}/edition"),
        json!({
            "request_id": Uuid::new_v4(), "expected_revision": 0,
            "recording_id": recordings[0], "association_id": association,
            "release_group_id": album::GROUP_ID, "edition_id": album::EDITION_ID,
        }),
    )
    .await;
    assert_eq!(receipt["revision"], 1);
    assert_eq!(receipt["release_id"], release);
    assert_eq!(
        receipt["edition"],
        json!({
            "recording_id": recordings[0], "association_id": association,
            "release_group_id": album::GROUP_ID, "edition_id": album::EDITION_ID,
        })
    );
    assert_eq!(provider.requests().len(), before_choice);
    let (status, _) = account.request(app, "POST", "/api/v1/users", json!({
        "username": "library-stats-peer", "password": "owned library statistics peer password",
    })).await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        app,
        "library-stats-peer",
        "owned library statistics peer password",
    )
    .await;
    Selected {
        account,
        peer,
        installation,
        recordings,
        release,
        association,
    }
}

async fn read_saved(app: &Router, selected: &Selected, provider: &FakePlugin) -> Value {
    let Selected {
        account,
        peer,
        installation,
        recordings,
        release,
        association,
    } = selected;
    let before = snapshot(app, selected).await;
    let peer_before = peer.get(app, "/api/v1/recordings").await;
    let peer_plugins = peer.get(app, "/api/v1/plugins").await;
    let requests_before = provider.requests().len();
    let path = format!("/api/v1/albums/{release}/edition/track-list");
    let (status, headers, _) =
        review::exchange(app, "POST", &path, Value::Null, peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(provider.requests().len(), requests_before);
    let (status, headers, response) =
        review::exchange(app, "POST", &path, Value::Null, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    assert_eq!(response["status"], "available");
    assert_eq!(response["association_id"], *association);
    assert_eq!(response["edition"], catalog::edition());
    assert_eq!(response["is_album_release"], true);
    assert_eq!(response["attached_occurrences"], json!([]));
    assert_eq!(
        response["adoption"],
        json!({
            "installation_id": installation["id"], "installation_revision": installation["revision"],
        })
    );
    let matches = response["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| {
            (
                found["occurrence_id"].as_str().unwrap(),
                found["recording"]["id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches,
        [
            ("occurrence:library-stats/first", recordings[0].as_str()),
            ("occurrence:library-stats/second", recordings[1].as_str()),
            ("occurrence:library-stats/repeat", recordings[0].as_str()),
        ]
    );
    assert_eq!(
        response["matches"][1]["recording"]["title"],
        "Owned personal library title"
    );
    let library_only = before["album"]["tracks"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|track| track["id"] == recordings[2])
        .unwrap();
    assert_eq!(response["library_only"], json!([library_only]));
    assert_eq!(snapshot(app, selected).await, before);
    assert_eq!(peer.get(app, "/api/v1/recordings").await, peer_before);
    assert_eq!(peer.get(app, "/api/v1/plugins").await, peer_plugins);
    assert_eq!(provider.unexpected(), 0);
    let requests = provider.requests();
    let dispatched = &requests[requests_before..];
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[0].operation, catalog::CAPABILITIES);
    assert_eq!(dispatched[1].operation, catalog::TRACK_LIST);
    assert_eq!(
        dispatched[1].json(),
        json!({"release_group_id": album::GROUP_ID, "edition_id": album::EDITION_ID})
    );
    assert!(
        dispatched
            .iter()
            .all(|request| request.protocol().as_deref() == Some("1"))
    );
    response
}

async fn snapshot(app: &Router, selected: &Selected) -> Value {
    let Selected {
        account,
        recordings,
        release,
        ..
    } = selected;
    let mut metadata = Vec::new();
    for id in recordings {
        metadata.push(
            account
                .get(app, &format!("/api/v1/recordings/{id}/metadata"))
                .await,
        );
        assert_eq!(
            account
                .get(app, &format!("/api/v1/recordings/{id}/sources"))
                .await,
            json!([])
        );
        let (status, _) = account
            .request(
                app,
                "GET",
                &format!("/api/v1/recordings/{id}/download"),
                Value::Null,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    json!({
        "album": account.get(app, &format!("/api/v1/releases/{release}")).await,
        "catalog": account.get(app, "/api/v1/recordings").await,
        "releases": account.get(app, "/api/v1/releases").await,
        "artists": account.get(app, "/api/v1/artists").await,
        "libraries": account.get(app, "/api/v1/libraries").await,
        "activity": account.get(app, "/api/v1/activity").await,
        "events": account.get(app, "/api/v1/sync?cursor=0").await,
        "plugins": account.get(app, "/api/v1/plugins").await,
        "policy": account.get(app, "/api/v1/metadata/settings").await,
        "metadata": metadata,
    })
}

async fn accepted(app: &Router, account: &Account, method: &str, path: &str, body: Value) -> Value {
    let (status, value) = account.request(app, method, path, body).await;
    assert_eq!(status, StatusCode::OK);
    value
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
