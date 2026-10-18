#[path = "fixtures/release_credits_upgrade.rs"]
mod fixture;
#[path = "fixtures/content_service.rs"]
mod provider;
mod support;

use hito_core::{Browse, CollectionPublication, Error, ImportProviderPlaylist, Search, SyncEvent};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn populated_version_44_upgrade_preserves_release_state_and_rejects_broken_references() {
    let server = provider::ProviderServer::start().await;
    let legacy = fixture::seed(&server.url).await;
    let pool = fixture::connect(&legacy, true).await;
    assert_integrity(&pool).await;
    let before = snapshot_rows(&pool).await;
    assert_eq!(version(&pool).await, 44);
    pool.close().await;

    let (hito, actor) = legacy.open().await;
    let pool = fixture::connect(&legacy, true).await;
    assert_eq!(version(&pool).await, 45);
    assert_preserved(&before, &snapshot_rows(&pool).await);
    assert_integrity(&pool).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT seq FROM sqlite_sequence WHERE name='metadata_observations'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        2_000_000
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name='sync_events'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1900
    );
    let albums = hito
        .catalog
        .releases(
            &actor,
            Browse {
                q: Some("Pinned".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(albums.total, 2);
    for (id, title, credit, inherited_credit, source) in [
        (
            fixture::LOCAL_RELEASE,
            "Pinned local album",
            "Private local credit",
            "Local album credit",
            fixture::SOURCE,
        ),
        (
            fixture::PROVIDER_RELEASE,
            "Pinned provider album",
            "Private provider credit",
            "Provider album credit",
            fixture::PROVIDER_SOURCE,
        ),
    ] {
        let detail = hito
            .catalog
            .release(&actor, id, Browse::default())
            .await
            .unwrap();
        assert_eq!(detail.release.id, id);
        assert_eq!(detail.release.title, title);
        assert_eq!(detail.release.artist.as_deref(), Some(credit));
        assert_eq!(detail.tracks.total, 1);
        assert_eq!(detail.tracks.items[0].entry_id, source);
        assert!(detail.tracks.items[0].recording.available);
        let metadata = hito
            .catalog
            .release_metadata(&actor, id, Default::default())
            .await
            .unwrap();
        assert_eq!(metadata.artist.value.as_deref(), Some(credit));
        assert_eq!(metadata.artist.inherited.as_deref(), Some(inherited_credit));
        assert_eq!(metadata.artist.personal.as_deref(), Some(credit));
        assert!(metadata.artist.operation_id.is_some());
        let artist = hito
            .catalog
            .artist(
                &actor,
                detail.release.artist_id.as_deref().unwrap(),
                Browse::default(),
            )
            .await
            .unwrap();
        assert_eq!(artist.artist.name, credit);
        assert_eq!(artist.artist.release_count, 1);
    }
    let recording = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("Pinned recording".into()),
                liked: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(recording.total, 1);
    assert_eq!(recording.items[0].id, fixture::PROVIDER_RECORDING);
    assert_eq!(recording.items[0].artist, "Private performer");
    let metadata = hito
        .catalog
        .recording_metadata(&actor, fixture::PROVIDER_RECORDING, Default::default())
        .await
        .unwrap();
    assert_eq!(metadata.title.inherited, "Provider track");
    assert_eq!(metadata.artist.inherited, "Provider performer");
    let observation = metadata.selected_observation.unwrap();
    assert_eq!(observation.id, fixture::OBSERVATION);
    assert_eq!(
        observation.album_artist.as_deref(),
        Some("Provider album credit")
    );
    assert_eq!(
        support::audio_bytes(
            hito.catalog
                .audio_file(&actor, fixture::SOURCE)
                .await
                .unwrap()
        ),
        std::fs::read(&legacy.original).unwrap()
    );
    let playlist = hito
        .collection
        .playlist(&actor, fixture::PLAYLIST)
        .await
        .unwrap();
    assert_eq!(
        playlist
            .entries
            .iter()
            .map(|entry| (entry.position, entry.recording_id.as_deref()))
            .collect::<Vec<_>>(),
        [
            (0, Some(fixture::RECORDING)),
            (1, Some(fixture::PROVIDER_RECORDING))
        ]
    );
    let imported = hito
        .collection
        .playlist(&actor, fixture::IMPORTED_PLAYLIST)
        .await
        .unwrap();
    assert_eq!(imported.entries.len(), 2);
    assert_ne!(imported.entries[0].entry_id, imported.entries[1].entry_id);
    assert!(
        imported
            .entries
            .iter()
            .all(|entry| entry.recording_id.as_deref() == Some(fixture::PROVIDER_RECORDING))
    );
    let events = hito.collection.events(&actor, 0).await.unwrap();
    assert_eq!(events.operations.len(), 1);
    let SyncEvent::Publication {
        sequence,
        publication: CollectionPublication::PlaylistImported { snapshot, .. },
    } = &events.operations[0]
    else {
        panic!("the historical imported snapshot must remain readable");
    };
    assert_eq!(*sequence, 77);
    assert_eq!(snapshot.playlist_id.to_string(), fixture::IMPORTED_PLAYLIST);
    assert_eq!(
        snapshot.recordings[0]
            .metadata
            .release
            .as_ref()
            .unwrap()
            .artist
            .as_deref(),
        Some("Provider album credit")
    );
    let replay = hito
        .collection
        .import_provider_playlist(&actor, fixture::INSTALLATION, fixture::request())
        .await
        .unwrap();
    assert_eq!(replay.playlist_id.to_string(), fixture::IMPORTED_PLAYLIST);
    assert!(server.requests("v1/playlists/detail").await.is_empty());
    assert_preserved(&before, &snapshot_rows(&pool).await);

    server
        .reply(
            "v1/playlists/detail",
            json!({"id":"playlist/new","name":"New selection","snapshot":"new-one",
        "total_count":1,"attribution":{"label":"Owned provider"}}),
        )
        .await;
    server
        .reply(
            "v1/playlists/items",
            json!({"playlist_id":"playlist/new","snapshot":"new-one","next_cursor":null,
        "items":[{"occurrence_id":"new","item":{"kind":"recording","recording":{
            "id":"recording/new","title":"New track","artist":"New performer",
            "release":{"id":"album/new","title":"New unknown album"}}}}]}),
        )
        .await;
    let new_copy = hito
        .collection
        .import_provider_playlist(
            &actor,
            fixture::INSTALLATION,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: 1,
                playlist_reference: "https://provider.invalid/playlist/new".into(),
            },
        )
        .await
        .unwrap();
    let new_albums = hito
        .catalog
        .releases(
            &actor,
            Browse {
                q: Some("New unknown album".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(new_albums.total, 1);
    assert!(new_albums.items[0].artist.is_none() && new_albums.items[0].artist_id.is_none());
    let new_entries = hito
        .collection
        .playlist(&actor, &new_copy.playlist_id.to_string())
        .await
        .unwrap();
    assert!(
        hito.catalog
            .sources(
                &actor,
                new_entries.entries[0].recording_id.as_deref().unwrap()
            )
            .await
            .unwrap()
            .is_empty()
    );
    assert_integrity(&pool).await;
    pool.close().await;
    drop(hito);

    let rejected = fixture::seed(&server.url).await;
    let pool = fixture::connect(&rejected, false).await;
    sqlx::query("UPDATE source_metadata SET release_id='missing-release' WHERE source_id=?")
        .bind(fixture::SOURCE)
        .execute(&pool)
        .await
        .unwrap();
    let broken = snapshot_rows(&pool).await;
    pool.close().await;
    let migration =
        hito_core::migrate_existing(rejected.directory.path().join("hito.db"), 44).await;
    assert!(matches!(migration, Err(Error::Migration(_))));
    let pool = fixture::connect(&rejected, true).await;
    assert_eq!(version(&pool).await, 44);
    assert_preserved(&broken, &snapshot_rows(&pool).await);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT \"notnull\" FROM pragma_table_info('releases') WHERE name='artist'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pool.close().await;
}

fn assert_preserved(before: &[(&str, Vec<String>)], after: &[(&str, Vec<String>)]) {
    assert_eq!(before.len(), after.len());
    for ((table, rows), (actual_table, actual_rows)) in before.iter().zip(after) {
        assert_eq!(table, actual_table);
        assert!(
            rows == actual_rows,
            "the migration changed prior {table} rows or rowids"
        );
    }
}

async fn snapshot_rows(pool: &sqlx::SqlitePool) -> Vec<(&'static str, Vec<String>)> {
    let mut snapshots = Vec::new();
    for table in [
        "releases",
        "account_releases",
        "artists",
        "account_artists",
        "recordings",
        "sources",
        "local_files",
        "source_metadata",
        "source_artist_credits",
        "account_source_credits",
        "account_recording_credits",
        "metadata_observations",
        "provider_items",
        "provider_sources",
        "provider_source_state",
        "personal_recording_metadata",
        "personal_release_metadata",
        "likes",
        "listening_progress",
        "playlists",
        "playlist_entries",
        "playlist_imports",
        "playlist_import_entries",
        "playlist_import_receipts",
        "sync_events",
        "sqlite_sequence",
        "_sqlx_migrations",
    ] {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_xinfo(?) ORDER BY cid")
                .bind(table)
                .fetch_all(pool)
                .await
                .unwrap();
        let fields = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(",");
        let filter = if table == "_sqlx_migrations" {
            " WHERE version <= 44"
        } else {
            ""
        };
        let query =
            format!("SELECT json_array(quote(rowid),{fields}) FROM {table}{filter} ORDER BY rowid");
        snapshots.push((
            table,
            sqlx::query_scalar(&query).fetch_all(pool).await.unwrap(),
        ));
    }
    snapshots
}

async fn version(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn assert_integrity(pool: &sqlx::SqlitePool) {
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(pool)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(pool)
            .await
            .unwrap(),
        "ok"
    );
    for index in [
        "releases_fts",
        "account_releases_fts",
        "artists_fts",
        "account_artists_fts",
        "recordings_fts",
    ] {
        sqlx::query(&format!(
            "INSERT INTO {index}({index},rank) VALUES ('integrity-check',1)"
        ))
        .execute(pool)
        .await
        .unwrap();
    }
}
