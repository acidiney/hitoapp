#[path = "fixtures/playlist_service.rs"]
mod provider;
mod support;
#[path = "fixtures/playlist_follow_upgrade.rs"]
mod upgrade;

use hito_core::{
    Action, CoreConfig, Credentials, HostDeviceId, ImportProviderPlaylist, InstallPlugin,
    OfferPluginService, Operation, PluginCredential, PluginRefresh, RefreshPlugin, Search,
};
use serde_json::{Value, json};
use sqlx::Connection;
use uuid::Uuid;

const HIGH_WATER: i64 = 900_000;
const BEARER: &str = "owned-follow-upgrade-credential";

async fn public_reads(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
    playlist: Uuid,
    recording: &str,
) -> Value {
    json!({
        "libraries": hito.catalog.libraries(actor).await.unwrap(),
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "personal_search": hito.catalog.recordings(actor, Search { q: Some("personal upgrade title".into()), ..Default::default() }).await.unwrap(),
        "liked": hito.catalog.recordings(actor, Search { liked: Some(true), ..Default::default() }).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(actor, recording, Default::default()).await.unwrap(),
        "history": hito.collection.activity(actor).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "playlist": hito.collection.playlist(actor, &playlist.to_string()).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
        "plugins": hito.plugins.settings(actor).await.unwrap(),
    })
}

#[tokio::test]
async fn the_follow_upgrade_preserves_populated_imports_private_state_and_deleted_ledger_high_water()
 {
    let server = provider::ProviderServer::start().await;
    let (directory, hito, actor, shared, music) = support::library("follow-upgrade").await;
    support::write_wav(&music.join("owned-upgrade-original.wav"), 17);
    support::scan(&hito, &actor, &shared).await;
    let local = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let local_source = hito
        .catalog
        .sources(&actor, &local.id)
        .await
        .unwrap()
        .remove(0);
    let original_audio = support::audio_bytes(
        hito.catalog
            .audio_file(&actor, &local_source.id)
            .await
            .unwrap(),
    );
    server.reply("v1/manifest", json!({"id": "org.hito.follow-upgrade", "name": "Owned upgrade provider", "protocol_version": 1, "capabilities": ["playlists"]})).await;
    provider::playlist(&server, "playlist/retained", "before-upgrade", vec![
        json!({"occurrence_id": "first", "item": {"kind": "recording", "recording": {"id": "recording/retained", "title": "Provider upgrade recording", "artist": "Provider upgrade artist"}}}),
        json!({"occurrence_id": "unknown", "item": {"kind": "unknown"}}),
        json!({"occurrence_id": "duplicate", "item": {"kind": "recording", "recording": {"id": "recording/retained", "title": "Provider upgrade recording", "artist": "Provider upgrade artist"}}}),
    ]).await;
    let service = hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: String::new(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id.clone(),
                service_revision: service.revision,
                bearer_token: Some(BEARER.into()),
            },
        )
        .await
        .unwrap();
    let request_id = Uuid::new_v4();
    let request = || ImportProviderPlaylist {
        request_id,
        installation_revision: installation.revision,
        playlist_reference: "https://provider.invalid/private-upgrade-selection".into(),
    };
    let copied = hito
        .collection
        .import_provider_playlist(&actor, &installation.id, request())
        .await
        .unwrap();
    let detail = hito
        .collection
        .playlist(&actor, &copied.playlist_id.to_string())
        .await
        .unwrap();
    let recording = Uuid::parse_str(detail.entries[0].recording_id.as_ref().unwrap()).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![
            Action::PlaylistRename {
                playlist_id: copied.playlist_id,
                name: "Personal upgrade selection".into(),
            },
            Action::RecordingTitleSet {
                recording_id: recording,
                title: "Personal upgrade title".into(),
            },
            Action::RecordingArtistSet {
                recording_id: recording,
                artist: "Personal upgrade artist".into(),
            },
            Action::LikeSet {
                recording_id: recording,
                liked: true,
            },
            Action::ProgressSet {
                recording_id: recording,
                position_ms: 4200,
                speed: 1.25,
            },
        ],
    )
    .await;
    let other_user = hito
        .identity
        .create_user(
            &actor,
            Credentials {
                username: "other".into(),
                password: "other follow upgrade password".into(),
            },
        )
        .await
        .unwrap();
    let other_session = hito
        .identity
        .login(Credentials {
            username: "other".into(),
            password: "other follow upgrade password".into(),
        })
        .await
        .unwrap();
    let other = hito
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    let other_installation = hito
        .plugins
        .install(
            &other,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: Some("other-owned-upgrade-credential".into()),
            },
        )
        .await
        .unwrap();
    let other_copy = hito
        .collection
        .import_provider_playlist(
            &other,
            &other_installation.id,
            ImportProviderPlaylist {
                request_id,
                installation_revision: other_installation.revision,
                playlist_reference: "https://provider.invalid/other-upgrade-selection".into(),
            },
        )
        .await
        .unwrap();
    let other_recording = hito
        .collection
        .playlist(&other, &other_copy.playlist_id.to_string())
        .await
        .unwrap()
        .entries[0]
        .recording_id
        .clone()
        .unwrap();
    assert_ne!(other_recording, recording.to_string());
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();

    let source = directory.path().join("hito.db");
    let mut database = upgrade::connect(&source).await;
    let retired = Operation {
        id: Uuid::new_v4(),
        action: Action::PlaylistRename {
            playlist_id: copied.playlist_id,
            name: "Personal upgrade selection".into(),
        },
    };
    sqlx::query("INSERT INTO sync_events(sequence,user_id,origin,device_id,event_id,payload) VALUES (?,?,'client_operation','retired-fixture',?,?)")
        .bind(HIGH_WATER).bind(&actor.user().id).bind(retired.id.to_string()).bind(serde_json::to_string(&retired).unwrap()).execute(&mut database).await.unwrap();
    sqlx::query("DELETE FROM sync_events WHERE sequence = ?")
        .bind(HIGH_WATER)
        .execute(&mut database)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name = 'sync_events'")
            .fetch_one(&mut database)
            .await
            .unwrap(),
        HIGH_WATER
    );
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT max(sequence) FROM sync_events")
            .fetch_one(&mut database)
            .await
            .unwrap()
            < HIGH_WATER
    );
    sqlx::query("UPDATE recordings SET rowid = rowid * 10 + 7")
        .execute(&mut database)
        .await
        .unwrap();
    for (account, rowid) in [
        (actor.user().id.as_str(), 113_i64),
        (other.user().id.as_str(), 47_i64),
    ] {
        assert_eq!(
            sqlx::query(
                "UPDATE playlist_import_receipts SET rowid = ? WHERE user_id = ? AND request_id = ?",
            )
            .bind(rowid)
            .bind(account)
            .bind(request_id.to_string())
            .execute(&mut database)
            .await
            .unwrap()
            .rows_affected(),
            1
        );
    }
    let receipt_rowids: Vec<(String, i64)> = sqlx::query_as(
        "SELECT playlist_id,rowid FROM playlist_import_receipts ORDER BY playlist_id",
    )
    .fetch_all(&mut database)
    .await
    .unwrap();
    assert_eq!(receipt_rowids.len(), 2);
    database.close().await.unwrap();
    let before = public_reads(&hito, &actor, copied.playlist_id, &recording.to_string()).await;
    let other_before = public_reads(&hito, &other, other_copy.playlist_id, &other_recording).await;
    let receipt_before = serde_json::to_value(&copied).unwrap();
    let key = std::fs::read(source.with_extension("plugin-key")).unwrap();
    assert_eq!(key.len(), 32);
    let baseline_directory = directory.path().join("version-27");
    std::fs::create_dir(&baseline_directory).unwrap();
    let baseline = baseline_directory.join("hito.db");
    let tables = upgrade::baseline(&source, &baseline).await;
    drop(hito);

    let upgraded = hito_core::open(CoreConfig::new(
        baseline.clone(),
        music,
        HostDeviceId::new("follow-upgrade").unwrap(),
    ))
    .await
    .unwrap();
    let actor = upgraded
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let other = upgraded
        .identity
        .authenticate(&other_session.token)
        .await
        .unwrap();
    assert_eq!(other.user().id, other_user.id);
    assert_eq!(
        public_reads(
            &upgraded,
            &actor,
            copied.playlist_id,
            &recording.to_string()
        )
        .await,
        before
    );
    assert_eq!(
        public_reads(&upgraded, &other, other_copy.playlist_id, &other_recording).await,
        other_before
    );
    assert!(
        std::fs::read(baseline.with_extension("plugin-key")).unwrap() == key,
        "The private plugin key must not change during upgrade"
    );
    assert_eq!(
        support::audio_bytes(
            upgraded
                .catalog
                .audio_file(&actor, &local_source.id)
                .await
                .unwrap()
        ),
        original_audio
    );
    let mut database = upgrade::connect(&baseline).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations")
            .fetch_one(&mut database)
            .await
            .unwrap(),
        28
    );
    assert_eq!(
        sqlx::query_as::<_, (String, i64)>(
            "SELECT playlist_id,rowid FROM playlist_import_receipts ORDER BY playlist_id",
        )
        .fetch_all(&mut database)
        .await
        .unwrap(),
        receipt_rowids,
        "The Follow upgrade must preserve each prior import receipt's rowid"
    );
    upgrade::assert_preserved(&mut database, &tables).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name = 'sync_events'")
            .fetch_one(&mut database)
            .await
            .unwrap(),
        HIGH_WATER
    );

    let refresh = upgraded
        .plugins
        .refresh(
            &actor,
            &installation.id,
            RefreshPlugin {
                revision: installation.revision,
                accept_manifest_digest: None,
                credential: PluginCredential::Keep,
            },
        )
        .await
        .unwrap();
    assert!(matches!(refresh, PluginRefresh::Updated { .. }));
    server.offline().await;
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    let replay = upgraded
        .collection
        .import_provider_playlist(&actor, &installation.id, request())
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(replay).unwrap(), receipt_before);
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    support::synchronize(
        &upgraded,
        &actor,
        vec![Action::PlaylistRename {
            playlist_id: copied.playlist_id,
            name: "After follow upgrade".into(),
        }],
    )
    .await;
    let following = upgraded
        .collection
        .events(&actor, HIGH_WATER)
        .await
        .unwrap();
    assert_eq!(following.operations.len(), 1);
    assert!(
        following.operations[0].sequence() > HIGH_WATER,
        "New events must not reuse a deleted sequence"
    );
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sync_events WHERE sequence > ? AND origin = 'client_operation' AND device_id = 'catalog-test'").bind(HIGH_WATER).fetch_one(&mut database).await.unwrap(), 1);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut database)
            .await
            .unwrap()
            .is_empty()
    );
    database.close().await.unwrap();
}
