use hito_core::test_support::playlist_service as provider;
use hito_core::test_support::support;
#[path = "fixtures/playlist_metadata_upgrade.rs"]
mod upgrade;

use hito_core::{
    Action, Actor, Browse, CoreConfig, Credentials, Error, FollowProviderPlaylist, HostDeviceId,
    ImportProviderPlaylist, InstallPlugin, OfferPluginService, Operation, PlaylistFollowInterval,
    PlaylistFollowReceipt, PlaylistImportReceipt, PluginInstallation, Search, Services,
};
use serde_json::json;
use sqlx::Connection;
use uuid::Uuid;

const HOST: &str = "playlist-metadata-upgrade";
const REFERENCE: &str = "https://provider.invalid/private-upgrade-selection";
const HIGH_WATER: i64 = 900_000;

struct Account {
    actor: Actor,
    token: String,
    installation: PluginInstallation,
    request_id: Uuid,
    copied: PlaylistImportReceipt,
    followed: PlaylistFollowReceipt,
    recording: String,
}

impl Account {
    fn import_request(&self) -> ImportProviderPlaylist {
        ImportProviderPlaylist {
            request_id: self.request_id,
            installation_revision: self.installation.revision,
            playlist_reference: REFERENCE.into(),
        }
    }

    fn follow_request(&self) -> FollowProviderPlaylist {
        FollowProviderPlaylist {
            request_id: self.request_id,
            installation_revision: self.installation.revision,
            playlist_reference: REFERENCE.into(),
            interval: PlaylistFollowInterval::Daily,
        }
    }
}

#[tokio::test]
async fn metadata_upgrade_preserves_real_imports_follows_private_state_and_deleted_ledger_high_water()
 {
    let server = provider::ProviderServer::start().await;
    let (directory, hito, admin, shared, music) = support::library(HOST).await;
    let owned_audio = support::write_wav(&music.join("owned-upgrade-original.wav"), 19);
    support::scan(&hito, &admin, &shared).await;
    let local = hito
        .catalog
        .recordings(&admin, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let source = hito
        .catalog
        .sources(&admin, &local.id)
        .await
        .unwrap()
        .remove(0);
    let user = hito
        .identity
        .create_user(
            &admin,
            Credentials {
                username: "other-upgrade-account".into(),
                password: "other owned upgrade password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&admin, &shared, &user.id, true)
        .await
        .unwrap();
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.metadata-upgrade-fixture", "name": "Owned migration provider",
                "protocol_version": 1, "capabilities": ["playlists"]
            }),
        )
        .await;
    provider::playlist(&server, "playlist/retained", "before-metadata-upgrade", vec![
        json!({"occurrence_id": "first", "item": {"kind": "recording", "recording": {
            "id": "recording/retained", "title": "Inherited provider title", "artist": "Inherited performer",
            "duration_ms": 90_000, "revision": "before-albums"
        }}}),
        json!({"occurrence_id": "unknown", "item": {"kind": "unknown"}}),
        json!({"occurrence_id": "again", "item": {"kind": "recording", "recording": {
            "id": "recording/retained", "title": "Inherited provider title", "artist": "Inherited performer",
            "duration_ms": 90_000, "revision": "before-albums"
        }}}),
        json!({"occurrence_id": "album-member", "item": {"kind": "recording", "recording": {
            "id": "recording/with-album", "title": "Retained album member", "artist": "Album performer",
            "release": {"id": "album/retained", "title": "Retained searchable album", "artist": "Album performer"},
            "disc_number": 1, "track_number": 2, "revision": "retained-album-one"
        }}}),
    ]).await;
    let service = hito
        .plugins
        .offer_service(
            &admin,
            OfferPluginService {
                name: String::new(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let request_id = Uuid::new_v4();
    let mut accounts = Vec::new();
    for (index, username, password) in [
        (0, "admin", "indexing test password"),
        (1, "other-upgrade-account", "other owned upgrade password"),
    ] {
        let session = hito
            .identity
            .login(Credentials {
                username: username.into(),
                password: password.into(),
            })
            .await
            .unwrap();
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: Some(format!("owned-metadata-upgrade-credential-{index}")),
                },
            )
            .await
            .unwrap();
        let copied = hito
            .collection
            .import_provider_playlist(
                &actor,
                &installation.id,
                ImportProviderPlaylist {
                    request_id,
                    installation_revision: installation.revision,
                    playlist_reference: REFERENCE.into(),
                },
            )
            .await
            .unwrap();
        let followed = hito
            .collection
            .follow_provider_playlist(
                &actor,
                &installation.id,
                FollowProviderPlaylist {
                    request_id,
                    installation_revision: installation.revision,
                    playlist_reference: REFERENCE.into(),
                    interval: PlaylistFollowInterval::Daily,
                },
            )
            .await
            .unwrap();
        let detail = hito
            .collection
            .playlist(&actor, &copied.playlist_id.to_string())
            .await
            .unwrap();
        assert_ne!(detail.entries[0].entry_id, detail.entries[2].entry_id);
        assert_eq!(
            detail.entries[0].recording_id,
            detail.entries[2].recording_id
        );
        let recording = detail.entries[0].recording_id.clone().unwrap();
        let recording_id = Uuid::parse_str(&recording).unwrap();
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::PlaylistRename {
                    playlist_id: copied.playlist_id,
                    name: format!("Private copy {index}"),
                },
                Action::RecordingTitleSet {
                    recording_id,
                    title: format!("Private title {index}"),
                },
                Action::RecordingArtistSet {
                    recording_id,
                    artist: format!("Private performer {index}"),
                },
                Action::LikeSet {
                    recording_id,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id,
                    position_ms: 4_200 + index,
                    speed: 1.25,
                },
                Action::EntryMove {
                    playlist_id: copied.playlist_id,
                    entry_id: Uuid::parse_str(&detail.entries[0].entry_id).unwrap(),
                    position: 8,
                },
                Action::EntryRemove {
                    playlist_id: copied.playlist_id,
                    entry_id: Uuid::parse_str(&detail.entries[1].entry_id).unwrap(),
                },
            ],
        )
        .await;
        accounts.push(Account {
            actor,
            token: session.token,
            installation,
            request_id,
            copied,
            followed,
            recording,
        });
    }
    assert_ne!(accounts[0].recording, accounts[1].recording);
    for index in 0..accounts.len() {
        let account = &accounts[index];
        let other = &accounts[1 - index];
        assert!(matches!(
            hito.collection
                .playlist(&account.actor, &other.copied.playlist_id.to_string())
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            hito.catalog
                .recording_metadata(&account.actor, &other.recording, Default::default())
                .await,
            Err(Error::NotFound)
        ));
    }

    let source_database = directory.path().join("hito.db");
    let mut database = upgrade::connect(&source_database).await;
    for (index, account) in accounts.iter().enumerate() {
        for (table, rowid) in [
            ("playlist_import_receipts", 101 + index as i64 * 102),
            ("playlist_follow_receipts", 307 + index as i64 * 102),
        ] {
            assert_eq!(
                sqlx::query(&format!(
                    "UPDATE {table} SET rowid = ? WHERE user_id = ? AND request_id = ?"
                ))
                .bind(rowid)
                .bind(&account.actor.user().id)
                .bind(request_id.to_string())
                .execute(&mut database)
                .await
                .unwrap()
                .rows_affected(),
                1
            );
        }
    }
    let events: Vec<(i64, String)> =
        sqlx::query_as("SELECT sequence,payload FROM sync_events ORDER BY sequence")
            .fetch_all(&mut database)
            .await
            .unwrap();
    for (sequence, payload) in events {
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        sqlx::query("UPDATE sync_events SET payload = ? WHERE sequence = ?")
            .bind(serde_json::to_string_pretty(&payload).unwrap())
            .bind(sequence)
            .execute(&mut database)
            .await
            .unwrap();
    }
    let deleted = Operation {
        id: Uuid::new_v4(),
        action: Action::PlaylistRename {
            playlist_id: accounts[0].copied.playlist_id,
            name: "Deleted ledger fixture".into(),
        },
    };
    sqlx::query(
        "INSERT INTO sync_events(sequence,user_id,origin,device_id,event_id,payload)
        VALUES (?,?,'client_operation','retired-upgrade-client',?,?)",
    )
    .bind(HIGH_WATER)
    .bind(&admin.user().id)
    .bind(deleted.id.to_string())
    .bind(serde_json::to_string(&deleted).unwrap())
    .execute(&mut database)
    .await
    .unwrap();
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
    database.close().await.unwrap();
    let mut before = Vec::new();
    for account in &accounts {
        before.push(public_state(&hito, account).await);
    }
    let key = std::fs::read(source_database.with_extension("plugin-key")).unwrap();
    assert_eq!(key.len(), 32);
    let old_directory = directory.path().join("historical-45");
    std::fs::create_dir(&old_directory).unwrap();
    let old_database = old_directory.join("hito.db");
    let baseline = upgrade::historical_45(&source_database, &old_database).await;
    drop(hito);

    let upgraded = hito_core::open(CoreConfig::new(
        old_database.clone(),
        music,
        HostDeviceId::new(HOST).unwrap(),
    ))
    .await
    .expect("the real production upgrade must accept populated schema-45 receipts");
    let mut database = upgrade::connect(&old_database).await;
    upgrade::assert_preserved(
        &mut database,
        &baseline,
        upgrade::collection_artwork_schema::latest_version(),
    )
    .await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name = 'sync_events'")
            .fetch_one(&mut database)
            .await
            .unwrap(),
        HIGH_WATER
    );
    assert!(
        std::fs::read(old_database.with_extension("plugin-key")).unwrap() == key,
        "The upgrade must retain the private plugin key bytes"
    );
    for (index, account) in accounts.iter_mut().enumerate() {
        account.actor = upgraded
            .identity
            .authenticate(&account.token)
            .await
            .unwrap();
        assert!(
            public_state(&upgraded, account).await == before[index],
            "The upgrade changed a private account's public state"
        );
        assert!(
            support::audio_bytes(
                upgraded
                    .playback
                    .audio_file(&account.actor, &source.id)
                    .await
                    .unwrap()
            ) == owned_audio,
            "The upgrade changed the original local audio"
        );
    }
    server.offline().await;
    let detail_requests = server.requests("v1/playlists/detail").await.len();
    let item_requests = server.requests("v1/playlists/items").await.len();
    for (index, account) in accounts.iter().enumerate() {
        let copy = upgraded
            .collection
            .import_provider_playlist(
                &account.actor,
                &account.installation.id,
                account.import_request(),
            )
            .await
            .unwrap();
        let follow = upgraded
            .collection
            .follow_provider_playlist(
                &account.actor,
                &account.installation.id,
                account.follow_request(),
            )
            .await
            .unwrap();
        assert!(
            serde_json::to_vec(&copy).unwrap() == serde_json::to_vec(&account.copied).unwrap(),
            "The upgrade changed an exact import receipt replay"
        );
        assert!(
            serde_json::to_vec(&follow).unwrap() == serde_json::to_vec(&account.followed).unwrap(),
            "The upgrade changed an exact Follow receipt replay"
        );
        assert!(
            public_state(&upgraded, account).await == before[index],
            "Receipt replay changed retained private state"
        );
        let other = &accounts[1 - index];
        assert!(matches!(
            upgraded
                .collection
                .playlist(&account.actor, &other.copied.playlist_id.to_string())
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            upgraded
                .catalog
                .recording_metadata(&account.actor, &other.recording, Default::default())
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(
        server.requests("v1/playlists/detail").await.len(),
        detail_requests
    );
    assert_eq!(
        server.requests("v1/playlists/items").await.len(),
        item_requests
    );
    upgrade::assert_preserved(
        &mut database,
        &baseline,
        upgrade::collection_artwork_schema::latest_version(),
    )
    .await;
    support::synchronize(
        &upgraded,
        &accounts[0].actor,
        vec![Action::PlaylistRename {
            playlist_id: accounts[0].copied.playlist_id,
            name: "After metadata upgrade".into(),
        }],
    )
    .await;
    let following = upgraded
        .collection
        .events(&accounts[0].actor, HIGH_WATER)
        .await
        .unwrap();
    assert_eq!(following.operations.len(), 1);
    assert!(
        following.operations[0].sequence() > HIGH_WATER,
        "New client events must not reuse a deleted ledger sequence"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut database)
            .await
            .unwrap()
            .is_empty()
    );
    database.close().await.unwrap();
}

async fn public_state(hito: &Services, account: &Account) -> Vec<u8> {
    let position = hito
        .collection
        .saved_position(&account.actor, Uuid::parse_str(&account.recording).unwrap())
        .await
        .unwrap()
        .map(|position| json!({ "position_ms": position.position_ms }));
    let titles = hito
        .catalog
        .recordings(
            &account.actor,
            Search {
                q: Some("Private title".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        titles.total, 1,
        "Private title search must remain scoped to one account"
    );
    assert_eq!(titles.items[0].id, account.recording);
    let performers = hito
        .catalog
        .artists(
            &account.actor,
            Browse {
                q: Some("Private performer".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        performers.total, 1,
        "Private artist search must remain scoped to one account"
    );
    let albums = hito
        .catalog
        .releases(
            &account.actor,
            Browse {
                q: Some("Retained searchable album".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        albums.total, 1,
        "Album search must read the retained provider index"
    );
    serde_json::to_vec(&json!({
        "libraries": hito.catalog.libraries(&account.actor).await.unwrap(),
        "catalog": hito.catalog.recordings(&account.actor, Default::default()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(&account.actor, &account.recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(&account.actor, &account.recording).await.unwrap(),
        "activity": hito.collection.activity(&account.actor).await.unwrap(),
        "position": position,
        "copy": hito.collection.playlist(&account.actor, &account.copied.playlist_id.to_string()).await.unwrap(),
        "follow": hito.collection.playlist(&account.actor, &account.followed.playlist_id.to_string()).await.unwrap(),
        "events": hito.collection.events(&account.actor, 0).await.unwrap(),
        "plugins": hito.plugins.settings(&account.actor).await.unwrap(),
        "title_search": titles,
        "artist_search": performers,
        "album_search": albums,
    })).unwrap()
}
