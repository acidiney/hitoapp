#[path = "../../../tests/fixtures/content_service.rs"]
mod provider;
pub(super) use provider::ProviderServer;

use super::TRACKS;
use crate::{
    AdoptProviderRecording, Browse, Credentials, ImportProviderPlaylist, InstallPlugin,
    OfferPluginService, Services,
};
use serde_json::{Value, json};
use sqlx::{Connection, SqlitePool};
use uuid::Uuid;

pub(super) struct Expected {
    pub(super) release: String,
    pub(super) artist: String,
    pub(super) recordings: Vec<String>,
    pub(super) entries: Vec<String>,
    pub(super) current_source: String,
}

pub(super) async fn seed(hito: &Services, server: &ProviderServer) -> (String, String, Expected) {
    let owner_session = hito
        .identity
        .setup(Credentials {
            username: "release-work-owner".into(),
            password: "owned release query work password".into(),
        })
        .await
        .unwrap();
    let owner = hito
        .identity
        .authenticate(&owner_session.token)
        .await
        .unwrap();
    let foreign_credentials = || Credentials {
        username: "release-work-foreign".into(),
        password: "owned foreign release work password".into(),
    };
    hito.identity
        .create_user(&owner, foreign_credentials())
        .await
        .unwrap();
    let foreign_session = hito.identity.login(foreign_credentials()).await.unwrap();
    server
        .reply(
            "v1/manifest",
            json!({
                "id":"org.hito.release-query-work", "name":"Owned release query fixture",
                "protocol_version":1, "capabilities":["playlists","sources"]
            }),
        )
        .await;
    server
        .reply(
            "v1/playlists/detail",
            json!({
                "id":"playlist/work", "name":"Owned work selection", "snapshot":"work-one",
                "total_count":TRACKS, "attribution":{"label":"Owned release query fixture"}
            }),
        )
        .await;
    let items = (0..TRACKS)
        .rev()
        .map(|index| {
            json!({
                "occurrence_id":format!("occurrence/{index}"),
                "item":{"kind":"recording","recording":provider_recording(index)}
            })
        })
        .collect::<Vec<_>>();
    server.reply("v1/playlists/items", json!({
        "playlist_id":"playlist/work", "snapshot":"work-one", "items":items, "next_cursor":null
    })).await;
    let service = hito
        .plugins
        .offer_service(
            &owner,
            OfferPluginService {
                name: "Owned work fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = hito
        .plugins
        .install(
            &owner,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let imported = hito
        .collection
        .import_provider_playlist(
            &owner,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/work".into(),
            },
        )
        .await
        .unwrap();
    let playlist = hito
        .collection
        .playlist(&owner, &imported.playlist_id.to_string())
        .await
        .unwrap();
    assert_eq!(playlist.entries.len(), TRACKS);
    let recordings = playlist
        .entries
        .iter()
        .rev()
        .map(|entry| entry.recording_id.clone().unwrap())
        .collect::<Vec<_>>();
    server
        .reply("v1/recordings/detail", provider_recording(0))
        .await;
    server
        .reply(
            "v1/sources",
            json!({"items":[{"id":"audio/current","label":"Owned current audio","ready":true}]}),
        )
        .await;
    let adopted = hito
        .catalog
        .adopt_provider(
            &owner,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: "recording/000".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(adopted.id, recordings[0]);
    let source = hito.catalog.sources(&owner, &adopted.id).await.unwrap();
    assert_eq!(source.len(), 1);
    assert!(source[0].ready);
    let release = adopted.release_id.unwrap();
    let detail = hito
        .catalog
        .release(
            &owner,
            &release,
            Browse {
                limit: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(detail.tracks.total, TRACKS as i64);
    assert_eq!(
        detail
            .tracks
            .items
            .iter()
            .map(|track| track.recording.id.clone())
            .collect::<Vec<_>>(),
        recordings
    );
    let mut entries = detail
        .tracks
        .items
        .iter()
        .map(|track| track.entry_id.clone())
        .collect::<Vec<_>>();
    entries[0] = std::cmp::min(source[0].id.clone(), history_source(1));
    let artists = hito
        .catalog
        .artists(&owner, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Owned work performer");
    server.offline().await;
    (
        owner_session.token,
        foreign_session.token,
        Expected {
            release,
            artist: artists.items[0].id.clone(),
            recordings,
            entries,
            current_source: source[0].id.clone(),
        },
    )
}

fn provider_recording(index: usize) -> Value {
    json!({
        "id":format!("recording/{index:03}"), "title":format!("Owned work track {index:03}"),
        "artist":"Owned work performer", "duration_ms":1000,
        "release":{"id":"album/work","title":"Owned work album"}, "revision":"metadata-one"
    })
}

fn history_source(index: i64) -> String {
    format!("e3000000-0000-4000-8000-{index:012x}")
}

pub(super) async fn add_copies(pool: &SqlitePool, source: &str, first: i64, last: i64) {
    let mut connection = pool.acquire().await.unwrap();
    let mut transaction = connection.begin().await.unwrap();
    sqlx::raw_sql(
        "CREATE TEMP TABLE hito_release_read_copy_numbers(i INTEGER PRIMARY KEY);
        CREATE TEMP TABLE hito_release_read_copy_source(source_id TEXT PRIMARY KEY);",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("INSERT INTO hito_release_read_copy_source VALUES (?)")
        .bind(source)
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM n WHERE i<?) INSERT INTO hito_release_read_copy_numbers SELECT i FROM n")
        .bind(first).bind(last).execute(&mut *transaction).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO sources(id,recording_id,provider,provider_id,ready)
            SELECT printf('e3000000-0000-4000-8000-%012x',n.i),s.recording_id,'plugin',json_array(ps.binding_id,'history/'||n.i),0
            FROM hito_release_read_copy_numbers n CROSS JOIN hito_release_read_copy_source selected
            JOIN sources s ON s.id=selected.source_id JOIN provider_sources ps ON ps.source_id=s.id;
         INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation)
            SELECT printf('e3000000-0000-4000-8000-%012x',n.i),ps.binding_id,ps.installation_id,ps.recording_id,'history/'||n.i,ps.label,ps.generation-1
            FROM hito_release_read_copy_numbers n CROSS JOIN hito_release_read_copy_source selected
            JOIN provider_sources ps ON ps.source_id=selected.source_id;
         INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number,inspection_version,observation_id,provenance)
            SELECT printf('e3000000-0000-4000-8000-%012x',n.i),m.release_id,m.title,m.artist,m.duration_ms,m.disc_number,m.track_number,m.inspection_version,m.observation_id,m.provenance
            FROM hito_release_read_copy_numbers n CROSS JOIN hito_release_read_copy_source selected
            JOIN source_metadata m ON m.source_id=selected.source_id;
         INSERT INTO source_artist_credits(source_id,artist_id)
            SELECT printf('e3000000-0000-4000-8000-%012x',n.i),credit.artist_id
            FROM hito_release_read_copy_numbers n CROSS JOIN hito_release_read_copy_source selected
            JOIN source_artist_credits credit ON credit.source_id=selected.source_id;
         INSERT INTO account_source_credits(user_id,source_id,recording_id,release_id,artist_id)
            SELECT credit.user_id,printf('e3000000-0000-4000-8000-%012x',n.i),credit.recording_id,credit.release_id,credit.artist_id
            FROM hito_release_read_copy_numbers n CROSS JOIN hito_release_read_copy_source selected
            JOIN account_source_credits credit ON credit.source_id=selected.source_id;
         DROP TABLE hito_release_read_copy_numbers;
         DROP TABLE hito_release_read_copy_source;",
    ).execute(&mut *transaction).await.unwrap();
    transaction.commit().await.unwrap();
}

pub(super) async fn assert_fixture(pool: &SqlitePool, copies: i64) {
    let foreign_keys: bool = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(foreign_keys);
    let counts: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM recordings),(SELECT count(*) FROM metadata_observations),
            (SELECT count(*) FROM sources),(SELECT count(*) FROM provider_sources),
            (SELECT count(*) FROM source_metadata),(SELECT count(*) FROM source_artist_credits),
            (SELECT count(*) FROM account_source_credits),(SELECT count(*) FROM account_recording_credits)",
    ).fetch_one(pool).await.unwrap();
    assert_eq!(
        counts,
        (
            TRACKS as i64,
            TRACKS as i64,
            copies + 1,
            copies + 1,
            copies + 1,
            copies + 1,
            copies + 1,
            TRACKS as i64
        )
    );
    let historical: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM provider_sources ps JOIN provider_source_state state ON state.binding_id=ps.binding_id
         JOIN sources s ON s.id=ps.source_id WHERE ps.generation<state.generation AND s.ready=0",
    ).fetch_one(pool).await.unwrap();
    assert_eq!(historical, copies);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(pool)
            .await
            .unwrap()
            .is_empty()
    );
}

pub(super) async fn assert_statistics(pool: &SqlitePool, statistics: &str, analyzed_copies: i64) {
    let has_stat1: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='sqlite_stat1')")
            .fetch_one(pool)
            .await
            .unwrap();
    if statistics == "absent" {
        if has_stat1 {
            let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_stat1")
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(rows, 0);
        }
        let has_stat4: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='sqlite_stat4')",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if has_stat4 {
            let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_stat4")
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(rows, 0);
        }
    } else {
        assert!(has_stat1);
        let stat: String =
            sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='source_metadata_release'")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(
            stat.split_whitespace()
                .next()
                .unwrap()
                .parse::<i64>()
                .unwrap(),
            analyzed_copies + 1
        );
    }
}

pub(super) async fn prepare_statistics(pool: &SqlitePool, statistics: &str) {
    match statistics {
        "absent" => {
            sqlx::query("DELETE FROM sqlite_stat1")
                .execute(pool)
                .await
                .unwrap();
            let has_stat4: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='sqlite_stat4')",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if has_stat4 {
                sqlx::query("DELETE FROM sqlite_stat4")
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
        "fresh" => {
            sqlx::query("ANALYZE").execute(pool).await.unwrap();
        }
        _ => {}
    }
}
