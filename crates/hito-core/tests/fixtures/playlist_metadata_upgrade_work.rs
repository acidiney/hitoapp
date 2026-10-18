use crate::{provider, support};
use hito_core::{
    Action, Credentials, FollowProviderPlaylist, ImportProviderPlaylist, InstallPlugin,
    OfferPluginService, PlaylistFollowInterval,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Connection, QueryBuilder, Sqlite, SqliteConnection};
use std::path::PathBuf;
use uuid::Uuid;

pub const ACCOUNTS: i64 = 10;
pub const HIGH_WATER: i64 = 10_000_000;
const REFERENCE: &str = "https://provider.invalid/metadata-upgrade-work";

pub struct Seed {
    pub database: PathBuf,
    pub templates: Vec<Template>,
    _directory: tempfile::TempDir,
}

pub struct Template {
    account: String,
    installation: String,
    revision: i64,
    copied: Uuid,
    followed: Uuid,
    import_sequence: i64,
    follow_sequence: i64,
    rename: String,
}

pub async fn public_seed() -> Seed {
    let server = provider::ProviderServer::start().await;
    let (directory, hito, admin, _library, _music) =
        support::library("metadata-upgrade-work").await;
    server
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.metadata-upgrade-work", "name": "Owned migration work provider",
                "protocol_version": 1, "capabilities": ["playlists"]
            }),
        )
        .await;
    provider::playlist(&server, "playlist/work", "work-snapshot", vec![json!({
        "item": {"kind": "recording", "recording": {
            "id": "recording/work", "title": "Work recording", "artist": "Work performer",
            "release": {"id": "release/work", "title": "Work album", "artist": "Work performer"},
            "disc_number": 1, "track_number": 1
        }}
    })]).await;
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
    let database = directory.path().join("hito.db");
    let mut connection = super::upgrade::connect(&database).await;
    let mut templates = Vec::new();
    for index in 0..ACCOUNTS {
        let actor = if index == 0 {
            admin.clone()
        } else {
            let credentials = || Credentials {
                username: format!("metadata-work-account-{index}"),
                password: "owned metadata upgrade work password".into(),
            };
            hito.identity
                .create_user(&admin, credentials())
                .await
                .unwrap();
            let session = hito.identity.login(credentials()).await.unwrap();
            hito.identity.authenticate(&session.token).await.unwrap()
        };
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    service_id: service.id.clone(),
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let request_id = Uuid::new_v4();
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
        let recording_id =
            Uuid::parse_str(detail.entries[0].recording_id.as_deref().unwrap()).unwrap();
        support::synchronize(
            &hito,
            &actor,
            vec![
                Action::RecordingTitleSet {
                    recording_id,
                    title: format!("Private work title {index}"),
                },
                Action::RecordingArtistSet {
                    recording_id,
                    artist: format!("Private work performer {index}"),
                },
                Action::PlaylistRename {
                    playlist_id: copied.playlist_id,
                    name: format!("Private work copy {index}"),
                },
            ],
        )
        .await;
        let receipt_sequence = |table: &str| {
            format!("SELECT publication_sequence FROM {table} WHERE user_id = ? AND request_id = ?",)
        };
        let import_sequence = sqlx::query_scalar(&receipt_sequence("playlist_import_receipts"))
            .bind(&actor.user().id)
            .bind(request_id.to_string())
            .fetch_one(&mut connection)
            .await
            .unwrap();
        let follow_sequence = sqlx::query_scalar(&receipt_sequence("playlist_follow_receipts"))
            .bind(&actor.user().id)
            .bind(request_id.to_string())
            .fetch_one(&mut connection)
            .await
            .unwrap();
        let rename = sqlx::query_scalar(
            "SELECT payload FROM sync_events WHERE user_id = ? AND origin = 'client_operation'
             AND json_extract(payload,'$.action.kind') = 'playlist_rename' ORDER BY sequence DESC LIMIT 1",
        ).bind(&actor.user().id).fetch_one(&mut connection).await.unwrap();
        templates.push(Template {
            account: actor.user().id.clone(),
            installation: installation.id,
            revision: installation.revision,
            copied: copied.playlist_id,
            followed: followed.playlist_id,
            import_sequence,
            follow_sequence,
            rename,
        });
    }
    connection.close().await.unwrap();
    drop(hito);
    Seed {
        database,
        templates,
        _directory: directory,
    }
}

pub async fn grow(
    connection: &mut SqliteConnection,
    templates: &[Template],
    events: i64,
    receipts: i64,
) {
    let retained: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_events")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(events > retained && events < HIGH_WATER && receipts > 1);
    assert_eq!(templates.len() as i64, ACCOUNTS);
    let mut tx = connection.begin().await.unwrap();
    sqlx::query("CREATE TEMP TABLE hito_metadata_work_accounts(position INTEGER PRIMARY KEY,user_id TEXT NOT NULL,rename_payload TEXT NOT NULL)")
        .execute(&mut *tx).await.unwrap();
    for (position, template) in templates.iter().enumerate() {
        sqlx::query("INSERT INTO hito_metadata_work_accounts VALUES (?,?,?)")
            .bind(position as i64)
            .bind(&template.account)
            .bind(&template.rename)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query(
        "WITH RECURSIVE additions(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM additions WHERE i<?)
         INSERT INTO sync_events(user_id,origin,device_id,event_id,payload)
         SELECT account.user_id,'client_operation','metadata-upgrade-work',
            printf('80000000-0000-4000-8000-%012x',i),
            json_set(account.rename_payload,'$.id',printf('80000000-0000-4000-8000-%012x',i))
         FROM additions JOIN hito_metadata_work_accounts account ON account.position=(i-1)%10",
    )
    .bind(events - retained)
    .execute(&mut *tx)
    .await
    .unwrap();
    for (account, template) in templates.iter().enumerate() {
        for follow in [false, true] {
            let table = if follow {
                "playlist_follow_receipts"
            } else {
                "playlist_import_receipts"
            };
            let sequence = if follow {
                template.follow_sequence
            } else {
                template.import_sequence
            };
            let playlist = if follow {
                template.followed
            } else {
                template.copied
            }
            .to_string();
            let mut rows = Vec::new();
            for ordinal in 1..receipts {
                let identity =
                    (u128::from(follow) << 40) | ((account as u128) << 32) | ordinal as u128;
                let request_id = Uuid::from_u128(0x70000000_0000_4000_8000_000000000000 + identity);
                let request = if follow {
                    serde_json::to_vec(&(
                        "hito-playlist-follow:v1",
                        &template.installation,
                        FollowProviderPlaylist {
                            request_id,
                            installation_revision: template.revision,
                            playlist_reference: REFERENCE.into(),
                            interval: PlaylistFollowInterval::Daily,
                        },
                    ))
                    .unwrap()
                } else {
                    serde_json::to_vec(&(
                        "hito-playlist-import:v1",
                        &template.installation,
                        ImportProviderPlaylist {
                            request_id,
                            installation_revision: template.revision,
                            playlist_reference: REFERENCE.into(),
                        },
                    ))
                    .unwrap()
                };
                rows.push((
                    request_id.to_string(),
                    format!("{:x}", Sha256::digest(request)),
                ));
            }
            for chunk in rows.chunks(128) {
                let mut insert = QueryBuilder::<Sqlite>::new(format!(
                    "INSERT INTO {table}(user_id,request_id,request_digest,playlist_id,publication_sequence) ",
                ));
                insert.push_values(chunk, |mut row, (request, digest)| {
                    row.push_bind(&template.account)
                        .push_bind(request)
                        .push_bind(digest)
                        .push_bind(&playlist)
                        .push_bind(sequence);
                });
                insert.build().execute(&mut *tx).await.unwrap();
            }
        }
    }
    let deleted = "90000000-0000-4000-8000-000000000001";
    sqlx::query(
        "INSERT INTO sync_events(sequence,user_id,origin,device_id,event_id,payload)
        VALUES (?,?,'client_operation','metadata-upgrade-work',?,json_set(?,'$.id',?))",
    )
    .bind(HIGH_WATER)
    .bind(&templates[0].account)
    .bind(deleted)
    .bind(&templates[0].rename)
    .bind(deleted)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("DELETE FROM sync_events WHERE sequence = ?")
        .bind(HIGH_WATER)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DROP TABLE hito_metadata_work_accounts")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_population(connection, events, receipts).await;
}

pub async fn assert_population(connection: &mut SqliteConnection, events: i64, receipts: i64) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        ACCOUNTS
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sync_events")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        events
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT seq FROM sqlite_sequence WHERE name='sync_events'")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        HIGH_WATER
    );
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT max(sequence) FROM sync_events")
            .fetch_one(&mut *connection)
            .await
            .unwrap()
            < HIGH_WATER
    );
    for (table, origin) in [
        ("playlist_import_receipts", "playlist_import"),
        ("playlist_follow_receipts", "playlist_follow"),
    ] {
        let counts: Vec<(String, i64)> = sqlx::query_as(&format!(
            "SELECT user_id,count(*) FROM {table} GROUP BY user_id ORDER BY user_id"
        ))
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        assert_eq!(counts.len() as i64, ACCOUNTS);
        assert!(counts.iter().all(|(_, count)| *count == receipts));
        assert_eq!(sqlx::query_scalar::<_, i64>(&format!(
            "SELECT count(*) FROM {table} receipt
             JOIN sync_events event ON event.sequence=receipt.publication_sequence
             JOIN playlists playlist ON playlist.id=receipt.playlist_id
             WHERE event.origin=? AND event.user_id=receipt.user_id AND playlist.user_id=receipt.user_id
                AND json_extract(event.payload,'$.snapshot.playlist_id')=receipt.playlist_id",
        )).bind(origin).fetch_one(&mut *connection).await.unwrap(), ACCOUNTS*receipts);
    }
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sync_events
        WHERE json_valid(payload)=0 OR json_extract(payload,'$.id') IS NOT event_id AND origin='client_operation'
           OR json_extract(payload,'$.publication_id') IS NOT event_id AND origin!='client_operation'")
        .fetch_one(&mut *connection).await.unwrap(), 0);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(connection)
            .await
            .unwrap()
            .is_empty()
    );
}
