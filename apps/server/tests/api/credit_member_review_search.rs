use crate::{
    account::{Account, exchange},
    artist_review_fixture::origin_counts,
};
use axum::{
    Router,
    http::{HeaderMap, Method, StatusCode},
};
use futures_util::FutureExt;
use hito_core::{
    Action, ArtistReviewContext, CoreConfig, HostDeviceId, MetadataProvider, Operation,
    test_support::{
        artist_catalog_discovery_service as catalog, database_owner::DatabaseOwner,
        hang_guard::hang_guard, playlist_service as origin,
    },
};
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn a_featured_member_searches_distinct_namesakes_without_replacing_the_whole_choice() {
    let directory = Arc::new(tempfile::tempdir().unwrap());
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let database = DatabaseOwner::retain(directory.clone());
    let mut config = CoreConfig::new(
        directory.path().join("hito.db"),
        music,
        HostDeviceId::new("member-search-http").unwrap(),
    );
    database.attach(&mut config);
    let mut services = None;
    let mut origin = None;
    let mut provider = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        let (app, prepared) = hang_guard(
            "member search HTTP library and reviewed whole choice",
            Box::pin(async {
                services = Some(hito_core::open(config).await.unwrap());
                origin = Some(origin::ProviderServer::start().await);
                provider = Some(catalog::DiscoveryServer::start().await);
                let app = hito_server::router(services.as_ref().unwrap().clone());
                let prepared = Box::pin(prepare(
                    &app,
                    origin.as_ref().unwrap(),
                    provider.as_ref().unwrap(),
                ))
                .await;
                (app, prepared)
            }),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(exercise(
                &app,
                &prepared,
                origin.as_ref().unwrap(),
                provider.as_ref().unwrap(),
            )),
        )
        .await
        .expect("The member search HTTP workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    if let Some(origin) = origin.take() {
        origin.shutdown().await;
    }
    let provider_joined = match provider.as_mut() {
        Some(provider) => provider.finish().await,
        None => true,
    };
    let closed = if let Some(hito) = services.take() {
        let playback = hito.playback.close().await;
        let replicas = hito.replicas.close().await;
        let replication = hito.catalog_replication.close().await;
        drop(hito);
        [playback.is_ok(), replicas.is_ok(), replication.is_ok()]
    } else {
        [true; 3]
    };
    let retired = database.retire(Duration::from_secs(5)).await;
    assert!(
        provider_joined,
        "The owned catalog provider must join shutdown"
    );
    assert!(closed.into_iter().all(|closed| closed));
    assert!(retired.resources_retired && !retired.deadline_exceeded);
    assert_eq!(database.observe().active_resources(), 0);
    let result = match outcome {
        Ok(response) => response,
        Err(panic) => resume_unwind(panic),
    };
    assert_eq!(
        result.status,
        StatusCode::OK,
        "A current featured member must search its own provider candidates"
    );
    private(&result.headers);
    let page: ArtistSearchPage = serde_json::from_value(result.found["page"].clone()).unwrap();
    page.validate_for(&result.search).unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].artist.name, "Owned catalog ensemble");
    assert_eq!(page.items[0].artist.name, page.items[1].artist.name);
    assert_eq!(page.items[0].artist.id, catalog::FIRST_ARTIST);
    assert_eq!(page.items[1].artist.id, catalog::SECOND_ARTIST);
    assert_ne!(page.items[0].artist.id, page.items[1].artist.id);
    assert_eq!(page.snapshot.as_deref(), Some(catalog::SNAPSHOT));
    assert_eq!(page.total_count, Some(2));
    assert!(page.next_cursor.is_none());
    assert_eq!(
        result.found,
        json!({"status": "available", "context": result.context, "page": page})
    );
    assert_eq!(
        result.dispatched,
        vec![
            (
                Method::GET,
                "/prefix/v1/catalog/discovery/capabilities".to_owned(),
                Value::Null
            ),
            (
                Method::POST,
                "/prefix/v1/catalog/artists/search".to_owned(),
                json!(result.search)
            ),
        ]
    );
}

struct Prepared {
    owner: Account,
    peer: Account,
    recording: String,
    playlist: String,
    member: String,
    context: Value,
}

async fn prepare(
    app: &Router,
    origin: &origin::ProviderServer,
    provider: &catalog::DiscoveryServer,
) -> Prepared {
    let owner = Account::setup(app).await;
    origin
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.member-search-playlist", "name": "Owned member search playlist",
                "protocol_version": 1, "capabilities": ["playlists"]
            }),
        )
        .await;
    origin::playlist(
        origin,
        "owned-member-search",
        "owned-member-search:1",
        vec![json!({
            "occurrence_id": "owned-duet-appearance", "item": {"kind": "recording", "recording": {
                "id": "owned-member-search-duet", "title": "Owned duet", "artist": "Ada feat. Cy",
                "duration_ms": 100_000, "artists": [
                    {"name": "Ada", "role": "main", "join": " feat. "},
                    {"name": "Cy", "role": "featured"}
                ]
            }}
        })],
    )
    .await;
    let origin_installation = owner
        .connect(app, &origin.url, "Owned member metadata", None)
        .await;
    let (status, receipt) = owner.request(app, "POST",
        &format!("/api/v1/plugins/{}/playlists/import", origin_installation["id"].as_str().unwrap()),
        json!({"request_id": Uuid::new_v4(), "installation_revision": origin_installation["revision"],
            "playlist_reference": "https://provider.invalid/member-search"}),
    ).await;
    assert_eq!(status, StatusCode::OK);
    let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
    let copy = owner
        .get(app, &format!("/api/v1/playlists/{playlist}"))
        .await;
    assert_eq!(copy["entries"].as_array().unwrap().len(), 1);
    let recording = copy["entries"][0]["recording_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let recorded = Uuid::parse_str(&recording).unwrap();
    let operations = [
        Action::RecordingTitleSet {
            recording_id: recorded,
            title: "Private owned duet title".into(),
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
    let acknowledged = accepted(
        app,
        &owner,
        "POST",
        "/api/v1/sync",
        json!({
            "device_id": "member-search-http", "operations": operations,
        }),
    )
    .await;
    assert_eq!(
        acknowledged["acknowledged"],
        json!(
            operations
                .iter()
                .map(|operation| operation.id)
                .collect::<Vec<_>>()
        )
    );
    let metadata = owner
        .get(app, &format!("/api/v1/recordings/{recording}/metadata"))
        .await;
    assert!(metadata["artist"]["personal"].is_null());
    assert_eq!(metadata["credited_artists"].as_array().unwrap().len(), 2);
    let ada = &metadata["credited_artists"][0];
    let cy = &metadata["credited_artists"][1];
    assert_eq!(ada["name"], "Ada");
    assert_eq!(ada["role"], "main");
    assert_eq!(cy["name"], "Cy");
    assert_eq!(cy["role"], "featured");
    let member = cy["artist_id"].as_str().unwrap().to_owned();
    assert_ne!(ada["artist_id"], member);
    let installed = owner
        .connect(
            app,
            &provider.url,
            "Owned member artist catalog",
            Some(catalog::TOKEN),
        )
        .await;
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installed["id"].as_str().unwrap().into(),
    };
    let settings = owner.get(app, "/api/v1/metadata/settings").await;
    let (status, policy) = owner
        .request(
            app,
            "PUT",
            "/api/v1/metadata/settings",
            json!({
                "revision": settings["revision"], "enabled": true, "providers": [selected],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let whole_context = accepted(
        app,
        &owner,
        "POST",
        &format!("/api/v1/recordings/{recording}/artist-review/context"),
        json!({"provider": selected}),
    )
    .await;
    let captured: ArtistReviewContext = serde_json::from_value(whole_context.clone()).unwrap();
    assert_eq!(captured.account_id, owner.id);
    assert_eq!(captured.recording_id, recording);
    assert_eq!(json!(captured.artist_id), ada["artist_id"]);
    assert_eq!(
        json!(captured.observation_id),
        metadata["selected_observation"]["id"]
    );
    assert_eq!(
        captured.policy_revision,
        policy["revision"].as_i64().unwrap()
    );
    assert!(captured.source_id.is_none() && captured.metadata_binding.is_some());
    let association = accepted(
        app,
        &owner,
        "POST",
        &format!("/api/v1/recordings/{recording}/artist-review/confirm"),
        json!({"context": whole_context, "provider_artist_id": "owned:artist/ada"}),
    )
    .await;
    assert_eq!(association["artist_id"], ada["artist_id"]);
    let chosen = accepted(
        app,
        &owner,
        "PUT",
        &format!("/api/v1/recordings/{recording}/artist-identity"),
        json!({
            "request_id": Uuid::new_v4(), "expected_revision": 0,
            "association_id": association["id"], "context": whole_context,
        }),
    )
    .await;
    assert_eq!(chosen["revision"], 1);
    assert_eq!(chosen["selection"]["association_id"], association["id"]);
    let (status, created) = owner
        .request(
            app,
            "POST",
            "/api/v1/users",
            json!({
                "username": "member-search-peer", "password": "owned member search peer password",
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let peer = Account::login(
        app,
        "member-search-peer",
        "owned member search peer password",
    )
    .await;
    assert_eq!(created["id"], peer.id);
    let expected = json!({
        "account_id": captured.account_id, "recording_id": captured.recording_id,
        "library_id": captured.library_id, "artist_id": member, "credited_name": "Cy", "role": "featured",
        "source_id": captured.source_id, "observation_id": captured.observation_id,
        "artist_operation_id": captured.artist_operation_id, "access_grant_id": captured.access_grant_id,
        "metadata_binding": captured.metadata_binding, "provider": selected,
        "policy_revision": captured.policy_revision, "connection": captured.connection,
    });
    let context = accepted(
        app,
        &owner,
        "POST",
        &format!("/api/v1/recordings/{recording}/credit-members/{member}/artist-review/context"),
        json!({"provider": selected}),
    )
    .await;
    assert_eq!(context, expected);
    Prepared {
        owner,
        peer,
        recording,
        playlist,
        member,
        context,
    }
}

struct SearchResult {
    status: StatusCode,
    headers: HeaderMap,
    found: Value,
    context: Value,
    search: ArtistSearchRequest,
    dispatched: Vec<(Method, String, Value)>,
}

async fn exercise(
    app: &Router,
    prepared: &Prepared,
    origin: &origin::ProviderServer,
    provider: &catalog::DiscoveryServer,
) -> SearchResult {
    let before = snapshot(app, prepared).await;
    assert_eq!(before["choice"]["revision"], 1);
    assert_eq!(before["recordings"]["items"][0]["liked"], true);
    assert_eq!(
        before["metadata"]["title"]["personal"],
        "Private owned duet title"
    );
    assert_eq!(before["progress"][0]["position_ms"], 400);
    assert_eq!(before["sources"], json!([]));
    assert_eq!(before["groups"]["total"], 1);
    let peer_before = peer_snapshot(app, &prepared.peer).await;
    assert_eq!(peer_before["recordings"]["total"], 0);
    assert_eq!(peer_before["plugins"]["installations"], json!([]));
    let origin_before = origin_counts(origin).await;
    let provider_before = provider.requests().len();
    let path = format!(
        "/api/v1/recordings/{}/credit-members/{}/artist-review/search",
        prepared.recording, prepared.member
    );
    let search = ArtistSearchRequest {
        query: "Cy".into(),
        cursor: None,
        snapshot: None,
        limit: 2,
    };
    let body = json!({"context": prepared.context, "search": search});
    let (status, headers, denied) =
        exchange(app, "POST", &path, body.clone(), prepared.peer.headers()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    private(&headers);
    assert_eq!(denied, json!({"error": "Resource not found"}));
    assert_eq!(
        provider.requests().len(),
        provider_before,
        "Peer denial must precede catalog dispatch"
    );
    assert_eq!(origin_counts(origin).await, origin_before);
    let (status, headers, found) =
        exchange(app, "POST", &path, body, prepared.owner.headers()).await;
    let requests = provider.requests();
    let dispatched = requests[provider_before..]
        .iter()
        .map(|request| {
            (
                request.method.clone(),
                request.path.clone(),
                request.body.clone(),
            )
        })
        .collect();
    assert_eq!(snapshot(app, prepared).await, before);
    assert_eq!(peer_snapshot(app, &prepared.peer).await, peer_before);
    assert_eq!(
        provider.requests().len(),
        requests.len(),
        "State inspection must not dispatch catalog requests"
    );
    assert_eq!(origin_counts(origin).await, origin_before);
    assert_eq!(provider.unexpected(), 0);
    SearchResult {
        status,
        headers,
        found,
        context: prepared.context.clone(),
        search,
        dispatched,
    }
}

async fn snapshot(app: &Router, prepared: &Prepared) -> Value {
    let account = &prepared.owner;
    let recording = &prepared.recording;
    let (status, progress) = account
        .request(
            app,
            "POST",
            "/api/v1/recordings/progress",
            json!({"recording_ids": [recording]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    json!({
        "recordings": account.get(app, "/api/v1/recordings").await,
        "metadata": account.get(app, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources": account.get(app, &format!("/api/v1/recordings/{recording}/sources")).await,
        "playlist": account.get(app, &format!("/api/v1/playlists/{}", prepared.playlist)).await,
        "choice": account.get(app, &format!("/api/v1/recordings/{recording}/artist-identity")).await,
        "associations": account.get(app, &format!("/api/v1/recordings/{recording}/artist-associations")).await,
        "groups": account.get(app, "/api/v1/artist-identity-groups").await,
        "events": account.get(app, "/api/v1/sync?cursor=0").await,
        "progress": progress,
        "plugins": account.get(app, "/api/v1/plugins").await,
        "settings": account.get(app, "/api/v1/metadata/settings").await,
    })
}

async fn peer_snapshot(app: &Router, peer: &Account) -> Value {
    json!({
        "recordings": peer.get(app, "/api/v1/recordings").await,
        "playlists": peer.get(app, "/api/v1/playlists").await,
        "groups": peer.get(app, "/api/v1/artist-identity-groups").await,
        "plugins": peer.get(app, "/api/v1/plugins").await,
        "settings": peer.get(app, "/api/v1/metadata/settings").await,
    })
}

async fn accepted(app: &Router, account: &Account, method: &str, path: &str, body: Value) -> Value {
    let (status, headers, body) = exchange(app, method, path, body, account.headers()).await;
    assert_eq!(status, StatusCode::OK);
    private(&headers);
    body
}

fn private(headers: &HeaderMap) {
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).is_ok());
}
