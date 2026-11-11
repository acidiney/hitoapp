use crate::artist_catalog::erasure::{account_statements, recording_statements};

const LIBRARIES: &str = "(SELECT id FROM libraries WHERE owner_id=?1)";
const RECORDINGS: &str =
    "(SELECT r.id FROM recordings r JOIN libraries l ON l.id=r.library_id WHERE l.owner_id=?1)";
const RELEASES: &str =
    "(SELECT id FROM releases WHERE library_id IN (SELECT id FROM libraries WHERE owner_id=?1))";
const SOURCES: &str = "(SELECT id FROM sources WHERE recording_id IN (SELECT r.id FROM recordings r JOIN libraries l ON l.id=r.library_id WHERE l.owner_id=?1))";
const INSTALLATIONS: &str = "(SELECT id FROM plugin_installations WHERE user_id=?1)";
const PLAYLISTS: &str = "(SELECT id FROM playlists WHERE user_id=?1)";
const INTENTS: &str = "(SELECT id FROM download_intents WHERE account_id=?1)";
const RETENTIONS: &str = "(SELECT id FROM download_retentions WHERE account_id=?1)";
const EPISODES: &str = "(SELECT id FROM podcast_episodes WHERE user_id=?1)";
const RESOLUTIONS: &str = "(SELECT id FROM source_resolutions WHERE user_id=?1)";

fn delete_where(table: &str, condition: &str) -> String {
    format!("DELETE FROM {table} WHERE {condition}")
}

fn owned(tables: &[&str], column: &str) -> Vec<String> {
    tables
        .iter()
        .map(|table| delete_where(table, &format!("{column}=?1")))
        .collect()
}

fn within(tables: &[&str], column: &str, set: &str) -> Vec<String> {
    tables
        .iter()
        .map(|table| delete_where(table, &format!("{column} IN {set}")))
        .collect()
}

pub(super) fn statements() -> Vec<String> {
    let mut plan = vec![
        "INSERT INTO erased_replica_profiles(id) SELECT id FROM replica_connections WHERE user_id=?1 ON CONFLICT DO NOTHING".to_string(),
        "DELETE FROM replica_activations WHERE connection_id IN (SELECT id FROM replica_connections WHERE user_id=?1)".to_string(),
        "DELETE FROM replica_connections WHERE user_id=?1".to_string(),
        "DELETE FROM replica_catalog_heads WHERE install_id IN (SELECT id FROM replica_catalog_installs WHERE account_id=?1)".to_string(),
        "DELETE FROM replica_catalog_facts WHERE install_id IN (SELECT id FROM replica_catalog_installs WHERE account_id=?1)".to_string(),
        "DELETE FROM replica_catalog_installs WHERE account_id=?1".to_string(),
        "DELETE FROM replica_state WHERE remote_account_id=?1".to_string(),
        "INSERT INTO erased_download_files(file_id,attempt_id,host_device) SELECT id,attempt_id,host_device FROM download_files WHERE account_id=?1 AND state IN ('staging','ready','deleting') ON CONFLICT DO NOTHING".to_string(),
        format!("INSERT INTO correction_erasure_scopes(kind,subject) SELECT 'recording',id FROM {RECORDINGS}"),
    ];
    plan.extend(owned(&["recording_lyric_identities"], "user_id"));
    plan.extend(account_statements());
    plan.extend(recording_statements(RECORDINGS));
    plan.push(delete_where(
        "session_locks",
        "token_hash IN (SELECT token_hash FROM sessions WHERE user_id=?1)",
    ));
    plan.extend(owned(
        &["plugin_authorization_attempts", "sessions"],
        "user_id",
    ));
    plan.extend(downloads());
    plan.extend(podcasts());
    plan.extend(playlists());
    plan.extend(artwork_and_sources());
    plan.extend(personal_catalog());
    plan.extend(provider_content());
    plan.extend(unreferenced_genre_lookups());
    plan.push("DELETE FROM correction_erasure_scopes WHERE kind='recording'".to_string());
    plan.push("DELETE FROM library_members WHERE user_id=?1 AND library_id NOT IN (SELECT id FROM libraries WHERE owner_id=?1)".to_string());
    plan.extend(owned(&["libraries"], "owner_id"));
    plan.extend(owned(
        &["plugin_connections", "plugin_installations", "sync_events"],
        "user_id",
    ));
    plan.extend(owned(&["users"], "id"));
    plan
}

fn unreferenced_genre_lookups() -> Vec<String> {
    const UNREFERENCED: &str = "(entity='artist' AND mbid NOT IN (SELECT provider_artist_id FROM account_artist_identity_groups WHERE provider_kind='builtin_musicbrainz' AND user_id<>?1))
        OR (entity<>'artist' AND NOT EXISTS (SELECT 1 FROM recording_musicbrainz_observations o WHERE o.entity=musicbrainz_genre_lookups.entity AND o.mbid=musicbrainz_genre_lookups.mbid))";
    vec![
        format!(
            "DELETE FROM musicbrainz_genres WHERE (entity,mbid) IN (SELECT entity,mbid FROM musicbrainz_genre_lookups WHERE {UNREFERENCED})"
        ),
        format!("DELETE FROM musicbrainz_genre_lookups WHERE {UNREFERENCED}"),
    ]
}

fn downloads() -> Vec<String> {
    let mut plan = within(
        &["download_terminal_failures", "download_retirement_work"],
        "intent_id",
        INTENTS,
    );
    plan[1] = delete_where(
        "download_retirement_work",
        &format!("transfer_id IN {INTENTS}"),
    );
    plan.extend(within(
        &[
            "download_playlist_retry_waiting",
            "download_playlist_retry_work",
            "download_playlist_work",
            "download_playlist_members",
        ],
        "retention_id",
        RETENTIONS,
    ));
    plan.extend(owned(
        &[
            "download_playlist_receipts",
            "download_receipts",
            "download_jobs",
            "download_retentions",
        ],
        "account_id",
    ));
    plan.extend(within(
        &["download_playlist_changes"],
        "playlist_id",
        PLAYLISTS,
    ));
    plan.extend(owned(&["download_intents", "download_files"], "account_id"));
    plan
}

fn podcasts() -> Vec<String> {
    let mut plan = within(&["podcast_sources"], "episode_id", EPISODES);
    plan.extend(owned(
        &[
            "podcast_unsubscribe_receipts",
            "podcast_refresh_receipts",
            "podcast_subscription_receipts",
        ],
        "user_id",
    ));
    plan.push("UPDATE podcast_request_total SET receipt_count=max(0,receipt_count-coalesce((SELECT receipt_count FROM podcast_request_account_counts WHERE user_id=?1),0)) WHERE singleton=1".to_string());
    plan.extend(owned(
        &[
            "podcast_request_account_counts",
            "podcast_refresh_jobs",
            "podcast_episodes",
            "podcast_subscriptions",
        ],
        "user_id",
    ));
    plan
}

fn playlists() -> Vec<String> {
    let mut plan = owned(
        &[
            "audiobook_bookmarks",
            "audiobook_continuations",
            "audiobooks",
        ],
        "user_id",
    );
    plan.extend(within(
        &["playlist_follow_entries", "playlist_import_entries"],
        "playlist_id",
        PLAYLISTS,
    ));
    plan.extend(owned(
        &[
            "playlist_follow_refresh_receipts",
            "playlist_follow_receipts",
            "playlist_follow_jobs",
            "playlist_follows",
            "playlist_import_receipts",
            "playlist_metadata_refresh_receipts",
        ],
        "user_id",
    ));
    plan.extend(within(
        &["playlist_imports", "playlist_entries"],
        "playlist_id",
        PLAYLISTS,
    ));
    plan.extend(owned(&["playlist_artwork"], "user_id"));
    plan
}

fn artwork_and_sources() -> Vec<String> {
    let mut plan = owned(
        &[
            "recording_artwork",
            "release_artwork",
            "cover_art_lookups",
            "artwork_fetches",
            "artwork_assets",
            "playlists",
        ],
        "user_id",
    );
    plan.extend(within(
        &["automatic_source_decisions"],
        "resolution_id",
        RESOLUTIONS,
    ));
    plan.extend(owned(
        &[
            "source_resolution_receipts",
            "source_resolutions",
            "source_choice_receipts",
            "source_choice_reset_receipts",
            "recording_source_choices",
            "source_preparation_receipts",
            "source_preparations",
            "community_reports",
            "community_reporters",
        ],
        "user_id",
    ));
    plan
}

fn personal_catalog() -> Vec<String> {
    owned(
        &[
            "account_source_credits",
            "account_recording_credits",
            "account_recording_credit_members",
            "account_releases",
            "account_credit_keys",
            "account_artists",
            "account_metadata_policies",
            "account_metadata_automation",
            "artist_matching_jobs",
            "account_community_preferences",
            "account_history_preferences",
            "account_avatars",
            "account_source_preferences",
            "account_community_ranking_preferences",
            "account_pin_locks",
            "personal_recording_metadata",
            "personal_release_metadata",
            "recording_group_members",
            "recording_group_splits",
            "likes",
            "listening_history",
            "listening_progress",
            "jobs",
            "library_access_grants",
        ],
        "user_id",
    )
}

fn provider_content() -> Vec<String> {
    let mut plan = vec![delete_where(
        "provider_source_state",
        &format!(
            "binding_id IN (SELECT id FROM provider_items WHERE installation_id IN {INSTALLATIONS})"
        ),
    )];
    plan.extend(within(
        &["provider_sources"],
        "installation_id",
        INSTALLATIONS,
    ));
    plan.extend(within(
        &[
            "source_artist_credits",
            "source_genres",
            "source_metadata",
            "source_unavailability",
        ],
        "source_id",
        SOURCES,
    ));
    plan.extend(within(&["local_files"], "library_id", LIBRARIES));
    plan.extend(within(&["recording_evidence"], "recording_id", RECORDINGS));
    plan.extend(within(
        &["provider_items"],
        "installation_id",
        INSTALLATIONS,
    ));
    plan.extend(within(&["sources"], "recording_id", RECORDINGS));
    plan.push(delete_where(
        "metadata_observations",
        &format!(
            "recording_id IN {RECORDINGS} OR release_id IN {RELEASES} OR plugin_installation_id IN {INSTALLATIONS}"
        ),
    ));
    plan.extend(within(&["recordings"], "id", RECORDINGS));
    plan.extend(within(&["releases"], "id", RELEASES));
    plan.extend(within(
        &["artists", "music_folders"],
        "library_id",
        LIBRARIES,
    ));
    plan.extend(within(&["library_access_grants"], "library_id", LIBRARIES));
    plan
}

#[cfg(test)]
mod tests {
    use super::statements;
    use crate::store::Store;
    use std::collections::BTreeSet;

    const OUTSIDE_THE_ACCOUNT_GRAPH: [&str; 10] = [
        "correction_erasure_scopes",
        "erased_download_files",
        "erased_replica_profiles",
        "musicbrainz_genre_lookups",
        "musicbrainz_genres",
        "plugin_services",
        "podcast_request_total",
        "replica_base_playlists",
        "replica_outbox",
        "replica_received_events",
    ];

    fn touched_tables() -> BTreeSet<String> {
        statements()
            .iter()
            .filter_map(|statement| {
                let mut words = statement.split_whitespace();
                match (words.next(), words.next()) {
                    (Some("DELETE"), Some("FROM")) | (Some("INSERT"), Some("INTO")) => words.next(),
                    (Some("UPDATE"), table) => table,
                    _ => None,
                }
            })
            .map(|table| table.split('(').next().unwrap().to_owned())
            .collect()
    }

    struct Described {
        table: String,
        parents: Vec<String>,
        columns: Vec<String>,
    }

    async fn schema(store: &Store) -> Vec<Described> {
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_list WHERE schema='main' AND type='table' AND name NOT LIKE 'sqlite_%' AND name<>'_sqlx_migrations' ORDER BY name",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let mut described = Vec::new();
        for table in tables {
            let parents = sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_list(?)")
                .bind(&table)
                .fetch_all(&store.pool)
                .await
                .unwrap();
            let columns = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
                .bind(&table)
                .fetch_all(&store.pool)
                .await
                .unwrap();
            described.push(Described {
                table,
                parents,
                columns,
            });
        }
        described
    }

    #[tokio::test]
    async fn every_table_reaching_users_by_foreign_key_or_account_named_column_is_in_the_plan() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("plan.db"))
            .await
            .unwrap();
        let described = schema(&store).await;
        let mut reachable: BTreeSet<String> = ["users".to_owned()].into();
        loop {
            let before = reachable.len();
            for entry in &described {
                let names_account = entry.columns.iter().any(|column| {
                    ["user_id", "account_id", "owner_id", "author_id"].contains(&column.as_str())
                        || column.ends_with("_by")
                });
                if names_account
                    || entry
                        .parents
                        .iter()
                        .any(|parent| reachable.contains(parent))
                {
                    reachable.insert(entry.table.clone());
                }
            }
            if reachable.len() == before {
                break;
            }
        }
        let touched = touched_tables();
        let uncovered: Vec<&String> = reachable.difference(&touched).collect();
        assert!(
            uncovered.is_empty(),
            "account deletion must cover these tables: {uncovered:?}"
        );
        let outside: BTreeSet<String> = described
            .iter()
            .map(|entry| entry.table.clone())
            .filter(|table| !reachable.contains(table))
            .collect();
        let expected: BTreeSet<String> = OUTSIDE_THE_ACCOUNT_GRAPH
            .iter()
            .map(|table| (*table).to_owned())
            .collect();
        assert_eq!(
            outside, expected,
            "a table that no account can reach must be classified in this test"
        );
    }

    #[tokio::test]
    async fn the_plan_is_valid_sql_and_ends_with_the_account_row() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("plan.db"))
            .await
            .unwrap();
        let mut tx = store.pool.begin().await.unwrap();
        let plan = statements();
        assert_eq!(plan.last().unwrap(), "DELETE FROM users WHERE id=?1");
        for statement in &plan {
            sqlx::query(statement)
                .bind("nobody")
                .execute(&mut *tx)
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }
    }

    #[tokio::test]
    async fn erasing_installations_does_not_scan_other_accounts_lyric_identities() {
        use crate::{
            Credentials, DeleteAccount,
            test_support::{support, work_meter::WorkMeter},
        };
        use hito_plugin_sdk::{
            Capability, Manifest, PROTOCOL_VERSION, ProviderRecording, RecordingMetadata,
        };
        use sqlx::Row;
        use uuid::Uuid;

        let installation_delete = statements()
            .into_iter()
            .find(|statement| statement.starts_with("DELETE FROM plugin_installations "))
            .unwrap();
        let mut measured = Vec::new();
        let mut probes = Vec::new();
        for foreign_count in [64, 4096] {
            let (_directory, hito, actor, library, _music) =
                support::library("erasure-fk-work").await;
            let erased = hito
                .identity
                .create_user(
                    &actor,
                    Credentials {
                        username: "erased-fk-owner".into(),
                        password: "owned erased account password".into(),
                    },
                )
                .await
                .unwrap();
            let peer = hito
                .identity
                .create_user(
                    &actor,
                    Credentials {
                        username: "retained-fk-peer".into(),
                        password: "owned retained account password".into(),
                    },
                )
                .await
                .unwrap();
            hito.identity
                .set_library_access(&actor, &library, &peer.id, true)
                .await
                .unwrap();
            let pool = &hito.identity.store.pool;
            let manifest = serde_json::to_string(&Manifest {
                id: "org.hito.owned-erasure-fixture".into(),
                name: "Owned erasure fixture".into(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: [Capability::Search, Capability::Metadata, Capability::Lyrics].into(),
                authorization: None,
                content_policy: None,
            })
            .unwrap();
            let peer_installation = Uuid::new_v4().to_string();
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            for index in 0..4 {
                let service = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO plugin_services(id,name,base_url) VALUES (?,'Owned FK fixture',?)",
                )
                .bind(&service)
                .bind(format!("http://127.0.0.1:{}/", 32000 + index))
                .execute(&mut *tx)
                .await
                .unwrap();
                sqlx::query("INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES (?,?,?,?)")
                    .bind(Uuid::new_v4().to_string()).bind(&erased.id).bind(&service).bind(&manifest)
                    .execute(&mut *tx).await.unwrap();
                if index == 0 {
                    sqlx::query("INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES (?,?,?,?)")
                        .bind(&peer_installation).bind(&peer.id).bind(&service).bind(&manifest)
                        .execute(&mut *tx).await.unwrap();
                }
            }
            let mut recording = String::new();
            for index in 0..foreign_count {
                if index % 8 == 0 {
                    recording = Uuid::new_v4().to_string();
                    sqlx::query("INSERT INTO recordings(id,library_id,title,artist) VALUES (?,?,'Owned peer recording','Owned peer artist')")
                        .bind(&recording).bind(&library).execute(&mut *tx).await.unwrap();
                }
                let provider_id = format!("owned-peer-recording-{index}");
                let metadata = RecordingMetadata {
                    recording: ProviderRecording {
                        id: provider_id.clone(),
                        title: "Owned peer recording".into(),
                        artist: "Owned peer artist".into(),
                        duration_ms: None,
                        release: None,
                        disc_number: None,
                        track_number: None,
                        revision: None,
                        attribution: None,
                        isrc: None,
                        artist_urls: Vec::new(),
                        artists: Vec::new(),
                    },
                    musical_title: None,
                    album_title: None,
                    source_ids: None,
                };
                metadata.validate().unwrap();
                sqlx::query("INSERT INTO recording_lyric_identities(id,user_id,recording_id,installation_id,provider_recording_id,reviewed_metadata) VALUES (?,?,?,?,?,?)")
                    .bind(Uuid::new_v4().to_string()).bind(&peer.id).bind(&recording)
                    .bind(&peer_installation).bind(provider_id).bind(serde_json::to_string(&metadata).unwrap())
                    .execute(&mut *tx).await.unwrap();
            }
            tx.commit().await.unwrap();
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            sqlx::query("PRAGMA defer_foreign_keys = ON")
                .execute(&mut *tx)
                .await
                .unwrap();
            for pragma in ["PRAGMA foreign_keys", "PRAGMA defer_foreign_keys"] {
                let enabled: i64 = sqlx::query_scalar(pragma)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
                assert_eq!(enabled, 1);
            }
            let plan = sqlx::query(&format!("EXPLAIN QUERY PLAN {installation_delete}"))
                .bind(&erased.id)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
            let references: Vec<String> = plan
                .iter()
                .map(|row| row.get::<String, _>("detail"))
                .filter(|detail| detail.contains("recording_lyric_identities"))
                .collect();
            assert!(
                !references.is_empty(),
                "The actual parent DELETE must check lyric foreign keys"
            );
            probes.extend(references);
            tx.rollback().await.unwrap();
            let meter = WorkMeter::install(pool).await;
            let (result, work) = meter
                .measure(hito.identity.delete_account(
                    &actor,
                    &erased.id,
                    DeleteAccount {
                        username: erased.username.clone(),
                    },
                ))
                .await;
            meter.remove(pool).await;
            result.unwrap();
            measured.push(work);
            let retained: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM recording_lyric_identities WHERE user_id=?",
            )
            .bind(&peer.id)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(retained, foreign_count);
            let erased_installations: i64 =
                sqlx::query_scalar("SELECT count(*) FROM plugin_installations WHERE user_id=?")
                    .bind(&erased.id)
                    .fetch_one(pool)
                    .await
                    .unwrap();
            assert_eq!(erased_installations, 0);
        }
        eprintln!(
            "Account erasure lyric FK VM_steps small={} large={}",
            measured[0], measured[1]
        );
        assert!(
            probes
                .iter()
                .all(|detail| detail.contains("SEARCH") && !detail.contains("SCAN")),
            "Parent installation deletion must probe referenced identities selectively rather than scan other accounts: {probes:?}"
        );
        assert!(
            measured[1] <= measured[0] + 2000,
            "Deleting one account used {} VM steps beside 4096 foreign identities after {} beside 64",
            measured[1],
            measured[0]
        );
    }
}
