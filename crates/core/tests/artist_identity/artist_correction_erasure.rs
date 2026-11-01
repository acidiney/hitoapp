use super::artist_correction_promotion::library::Library;
use super::artist_correction_promotion::{
    M1, M2, account, choose, link, listed, promote, run, unlinked,
};
use hito_core::{
    Actor, Credentials, Error, MetadataProvider, SplitArtistIdentityGroup, SplitDestination,
    UndoRecordingArtistChange, UndoSharedArtistCorrection, UnlinkRecordingArtist,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

fn id(actor: &Actor) -> &str {
    &actor.user().id
}

const ACCOUNT_TABLES: [&str; 18] = [
    "recording_artist_associations",
    "recording_artist_association_changes",
    "recording_artist_association_receipts",
    "recording_artist_association_change_order",
    "artist_association_change_retirements",
    "artist_identity_changes",
    "artist_identity_choice_receipts",
    "album_edition_choice_receipts",
    "shared_artist_correction_receipts",
    "recording_artist_identity_choices",
    "account_artist_identity_groups",
    "artist_match_dismissals",
    "artist_match_checks",
    "artist_provider_checks",
    "artist_name_checks",
    "artist_name_candidates",
    "artist_review_dismissals",
    "album_edition_choices",
];

const THROUGH_ASSOCIATION: [&str; 6] = [
    "artist_association_matches",
    "artist_association_provider_matches",
    "artist_association_name_matches",
    "artist_association_coverage_matches",
    "artist_association_recording_id_matches",
    "artist_association_candidate_confirmations",
];

async fn connect(library: &Library) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(library.database())
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

async fn count(connection: &mut SqliteConnection, sql: &str, account: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(account)
        .fetch_one(connection)
        .await
        .unwrap()
}

async fn owned(connection: &mut SqliteConnection, account: &str) -> Vec<(String, i64)> {
    let mut rows = Vec::new();
    for table in ACCOUNT_TABLES {
        let n = count(
            connection,
            &format!("SELECT count(*) FROM {table} WHERE user_id=?"),
            account,
        )
        .await;
        rows.push((table.to_string(), n));
    }
    for table in THROUGH_ASSOCIATION {
        let n = count(
            connection,
            &format!(
                "SELECT count(*) FROM {table} t JOIN recording_artist_associations a ON a.id=t.association_id WHERE a.user_id=?"
            ),
            account,
        )
        .await;
        rows.push((table.to_string(), n));
    }
    let items = count(
        connection,
        "SELECT count(*) FROM artist_identity_change_items i JOIN artist_identity_changes c ON c.id=i.change_id WHERE c.user_id=?",
        account,
    )
    .await;
    rows.push(("artist_identity_change_items".into(), items));
    rows
}

async fn others(connection: &mut SqliteConnection, account: &str) -> Vec<(String, i64)> {
    let mut rows = Vec::new();
    for table in ACCOUNT_TABLES {
        let n = count(
            connection,
            &format!("SELECT count(*) FROM {table} WHERE user_id<>?"),
            account,
        )
        .await;
        rows.push((table.to_string(), n));
    }
    rows
}

async fn guard_is_empty(connection: &mut SqliteConnection) {
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM correction_erasure_scopes")
        .fetch_one(connection)
        .await
        .unwrap();
    assert_eq!(scopes, 0, "No erasure scope outlives its transaction");
}

async fn add_provenance(connection: &mut SqliteConnection, account: &Actor, recording: &str) {
    let user = id(account);
    let association: String = sqlx::query_scalar(
        "SELECT id FROM recording_artist_associations WHERE user_id=? AND recording_id=? ORDER BY created_at,id LIMIT 1",
    )
    .bind(user)
    .bind(recording)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    let receipt: i64 = sqlx::query_scalar(
        "SELECT min(acceptance_order) FROM artist_identity_choice_receipts WHERE user_id=?",
    )
    .bind(user)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    for (table, kind) in [
        ("artist_association_matches", "file_tag_artist_id"),
        ("artist_association_provider_matches", "isrc"),
        ("artist_association_name_matches", "name_discography"),
    ] {
        sqlx::query(&format!(
            "INSERT INTO {table}(association_id,evidence_kind,receipt_order) VALUES(?,?,?)"
        ))
        .bind(&association)
        .bind(kind)
        .bind(receipt)
        .execute(&mut *connection)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO artist_association_coverage_matches(association_id,matched_tracks,probed_tracks,receipt_order) VALUES(?,5,6,?)")
        .bind(&association)
        .bind(receipt)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO artist_association_recording_id_matches(association_id,receipt_order) VALUES(?,?)")
        .bind(&association)
        .bind(receipt)
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO artist_association_candidate_confirmations(association_id,evidence_kind,receipt_order) VALUES(?,'user_confirmed_candidate',?)")
        .bind(&association)
        .bind(receipt)
        .execute(&mut *connection)
        .await
        .unwrap();
    let group: String =
        sqlx::query_scalar("SELECT id FROM account_artist_identity_groups WHERE user_id=? LIMIT 1")
            .bind(user)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    for statement in [
        "INSERT INTO artist_match_dismissals(user_id,artist_id,group_id) VALUES(?1,'dismissed',?2)",
        "INSERT INTO artist_match_checks(user_id,recording_id,observation_id) VALUES(?1,'r','o')",
        "INSERT INTO artist_provider_checks(user_id,recording_id,observation_id) VALUES(?1,'r','o')",
        "INSERT INTO artist_name_checks(user_id,recording_id,observation_id) VALUES(?1,'r','o')",
        "INSERT INTO artist_name_candidates(user_id,artist_id,provider_artist_id,position,name,corroborated_titles) VALUES(?1,'credit','mb',0,'Name',0)",
        "INSERT INTO artist_review_dismissals(user_id,artist_id,provider_artist_id) VALUES(?1,'credit','mb')",
        "INSERT INTO album_edition_choices(user_id,release_id,revision) VALUES(?1,'release',1)",
        "INSERT INTO album_edition_choice_receipts(user_id,request_id,request_digest,receipt) VALUES(?1,'request','digest','{}')",
        "INSERT INTO shared_artist_correction_receipts(user_id,request_id,request_digest,receipt) VALUES(?1,'request','digest','{}')",
    ] {
        sqlx::query(statement)
            .bind(user)
            .bind(&group)
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO artist_association_change_retirements(user_id,association_id,reason) VALUES(?,?,'credit_changed')")
        .bind(user)
        .bind(&association)
        .execute(&mut *connection)
        .await
        .unwrap();
}

async fn unlink_as(library: &Library, actor: &Actor, recording: &str, association: &str) {
    let artist_catalog = &library.hito.artist_catalog;
    let revision = artist_catalog
        .recording_artist_association_changes(actor, recording)
        .await
        .unwrap()
        .revision;
    artist_catalog
        .unlink_recording_artist(
            actor,
            recording,
            UnlinkRecordingArtist {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
                association_id: Uuid::parse_str(association).unwrap(),
            },
        )
        .await
        .unwrap();
    let revision = artist_catalog
        .recording_artist_association_changes(actor, recording)
        .await
        .unwrap()
        .revision;
    artist_catalog
        .undo_recording_artist_change(
            actor,
            recording,
            UndoRecordingArtistChange {
                request_id: Uuid::new_v4(),
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
}

struct People {
    peer: Actor,
    second: Actor,
}

async fn populate(library: &Library) -> People {
    let (admin, peer, local) = (&library.actor, &library.peer, &library.local);
    let second = account(library, "second-peer").await;
    for actor in [admin, peer, &second] {
        let first = choose(library, actor, local, M1).await;
        link(library, actor, local, M2).await;
        library
            .hito
            .artist_catalog
            .split_artist_identity_group(
                actor,
                &first,
                SplitArtistIdentityGroup {
                    request_id: Uuid::new_v4(),
                    expected_revision: 0,
                    recordings: vec![Uuid::parse_str(local).unwrap()],
                    releases: vec![],
                    destination: SplitDestination::Identity {
                        provider: MetadataProvider::BuiltinMusicBrainz,
                        provider_artist_id: M2.into(),
                    },
                },
            )
            .await
            .unwrap();
    }
    let association = link(library, peer, local, M1).await;
    unlink_as(library, peer, local, &association.id).await;
    let mut connection = connect(library).await;
    for actor in [admin, peer, &second] {
        add_provenance(&mut connection, actor, local).await;
    }
    connection.close().await.unwrap();
    People {
        peer: peer.clone(),
        second,
    }
}

async fn make_administrator(library: &Library, name: &str) -> Actor {
    let user = library
        .hito
        .identity
        .create_user(
            &library.actor,
            Credentials {
                username: name.into(),
                password: "owned correction peer password".into(),
            },
        )
        .await
        .unwrap();
    library
        .hito
        .identity
        .set_library_access(&library.actor, &library.shared, &user.id, true)
        .await
        .unwrap();
    let mut connection = connect(library).await;
    sqlx::query("UPDATE users SET admin=1 WHERE id=?")
        .bind(&user.id)
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    let session = library
        .hito
        .identity
        .login(Credentials {
            username: name.into(),
            password: "owned correction peer password".into(),
        })
        .await
        .unwrap();
    library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
}

#[tokio::test]
async fn erasing_an_account_removes_its_private_correction_history_and_only_that() {
    run(|library| {
        Box::pin(async move {
            let people = populate(library).await;
            let (peer, second) = (&people.peer, &people.second);
            let operator = make_administrator(library, "erasing-admin").await;
            let mut connection = connect(library).await;
            let peer_before = owned(&mut connection, id(peer)).await;
            assert!(
                peer_before.iter().all(|(_, n)| *n > 0),
                "The fixture gives the account every kind of correction row: {peer_before:?}"
            );
            let kept = others(&mut connection, id(peer)).await;
            let second_before = owned(&mut connection, id(second)).await;
            let second_listed = listed(library, second, &library.local).await;

            assert!(matches!(
                library
                    .hito
                    .artist_catalog
                    .erase_account_corrections(second, id(peer))
                    .await,
                Err(Error::Forbidden)
            ));
            assert!(matches!(
                library
                    .hito
                    .artist_catalog
                    .erase_account_corrections(&operator, &Uuid::new_v4().to_string())
                    .await,
                Err(Error::NotFound)
            ));
            assert_eq!(owned(&mut connection, id(peer)).await, peer_before);

            for _ in 0..2 {
                library
                    .hito
                    .artist_catalog
                    .erase_account_corrections(&operator, id(peer))
                    .await
                    .unwrap();
            }
            assert!(
                owned(&mut connection, id(peer))
                    .await
                    .iter()
                    .all(|(_, n)| *n == 0),
                "No private decision of the account remains"
            );
            assert_eq!(others(&mut connection, id(peer)).await, kept);
            assert_eq!(owned(&mut connection, id(second)).await, second_before);
            guard_is_empty(&mut connection).await;
            connection.close().await.unwrap();
            assert_eq!(listed(library, second, &library.local).await, second_listed);
            assert_eq!(listed(library, peer, &library.local).await, 0);
        })
    })
    .await;
}

#[tokio::test]
async fn ordinary_updates_and_deletes_of_retained_corrections_are_still_rejected() {
    run(|library| {
        Box::pin(async move {
            let people = populate(library).await;
            let peer = id(&people.peer).to_string();
            let mine = link(library, &library.actor, &library.local, M1).await;
            super::artist_correction_promotion::unlink(library, &library.local, &mine.id).await;
            library
                .hito
                .artist_catalog
                .promote_artist_correction(
                    &library.actor,
                    promote(0, unlinked(&library.local, &mine.id)),
                )
                .await
                .unwrap();
            let mut connection = connect(library).await;
            let before = owned(&mut connection, &peer).await;
            let deletes = [
                ("recording_artist_association_changes", "user_id"),
                ("recording_artist_association_receipts", "user_id"),
                ("recording_artist_association_change_order", "user_id"),
                ("artist_association_change_retirements", "user_id"),
                ("artist_identity_choice_receipts", "user_id"),
                ("album_edition_choice_receipts", "user_id"),
                ("shared_artist_correction_receipts", "user_id"),
                ("recording_artist_associations", "user_id"),
            ];
            let mut guarded = Vec::new();
            for scope in [None, Some(("account", Uuid::new_v4().to_string()))] {
                let mut tx = connection.begin().await.unwrap();
                if let Some((kind, subject)) = &scope {
                    let inserted = sqlx::query(
                        "INSERT INTO correction_erasure_scopes(kind,subject) VALUES(?,?)",
                    )
                    .bind(kind)
                    .bind(subject)
                    .execute(&mut *tx)
                    .await;
                    assert!(inserted.is_ok(), "The narrow erasure scope table exists");
                }
                for (table, column) in deletes {
                    let result = sqlx::query(&format!("DELETE FROM {table} WHERE {column}=?"))
                        .bind(&peer)
                        .execute(&mut *tx)
                        .await;
                    guarded.push((table, result.is_err()));
                }
                for table in THROUGH_ASSOCIATION {
                    let result = sqlx::query(&format!("DELETE FROM {table}"))
                        .execute(&mut *tx)
                        .await;
                    guarded.push((table, result.is_err()));
                }
                tx.rollback().await.unwrap();
            }
            assert!(
                guarded.iter().all(|(_, rejected)| *rejected),
                "A missing or foreign erasure scope never permits a delete: {guarded:?}"
            );
            for (table, statement) in [
                ("recording_artist_association_changes", "SET linked=linked"),
                ("recording_artist_associations", "SET revision=1"),
                ("artist_identity_choice_receipts", "SET receipt=receipt"),
                ("shared_artist_corrections", "SET promoted_by=NULL"),
                ("shared_artist_correction_states", "SET changed_by=NULL"),
                (
                    "artist_association_matches",
                    "SET receipt_order=receipt_order",
                ),
            ] {
                let result = sqlx::query(&format!("UPDATE {table} {statement}"))
                    .execute(&mut connection)
                    .await;
                assert!(result.is_err(), "{table} stays immutable");
            }
            assert_eq!(owned(&mut connection, &peer).await, before);
            guard_is_empty(&mut connection).await;
            connection.close().await.unwrap();
        })
    })
    .await;
}

#[tokio::test]
async fn a_deleted_administrators_shared_correction_stays_in_force_and_can_be_withdrawn() {
    run(|library| {
        Box::pin(async move {
            let (admin, local) = (&library.actor, &library.local);
            let people = populate(library).await;
            let (peer, second) = (&people.peer, &people.second);
            let operator = make_administrator(library, "other-admin").await;
            let mine = link(library, admin, local, M1).await;
            super::artist_correction_promotion::unlink(library, local, &mine.id).await;
            let shared = library
                .hito
                .artist_catalog
                .promote_artist_correction(admin, promote(0, unlinked(local, &mine.id)))
                .await
                .unwrap();
            link(library, second, local, M1).await;
            assert_eq!(listed(library, peer, local).await, 1);

            let mut connection = connect(library).await;
            let items: i64 = count(
                &mut connection,
                "SELECT count(*) FROM artist_identity_change_items i JOIN artist_identity_changes c ON c.id=i.change_id WHERE c.user_id=?",
                id(admin),
            )
            .await;
            assert!(items > 0);
            let peer_rows = owned(&mut connection, id(peer)).await;
            library
                .hito
                .artist_catalog
                .erase_account_corrections(&operator, id(admin))
                .await
                .unwrap();
            assert!(
                owned(&mut connection, id(admin))
                    .await
                    .iter()
                    .all(|(_, n)| *n == 0)
            );
            assert_eq!(owned(&mut connection, id(peer)).await, peer_rows);
            let anonymous: (i64, i64, i64) = sqlx::query_as(
                "SELECT count(*), sum(promoted_by IS NULL), sum(source_association_id IS NULL AND source_change_id IS NULL) FROM shared_artist_corrections",
            )
            .fetch_one(&mut connection)
            .await
            .unwrap();
            assert_eq!(anonymous, (1, 1, 1));
            let authors: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM shared_artist_correction_states WHERE changed_by IS NULL",
            )
            .fetch_one(&mut connection)
            .await
            .unwrap();
            assert_eq!(authors, 1);
            guard_is_empty(&mut connection).await;

            let list = library
                .hito
                .artist_catalog
                .shared_artist_corrections(&operator)
                .await
                .unwrap();
            assert_eq!(list.items.len(), 1);
            assert!(list.items[0].applied && !list.items[0].promoted_by_me);
            let by_former_author = library
                .hito
                .artist_catalog
                .shared_artist_corrections(admin)
                .await
                .unwrap();
            assert!(
                !by_former_author.items[0].promoted_by_me,
                "An anonymous author is nobody's own correction"
            );
            assert_eq!(listed(library, peer, local).await, 1, "Still in force");
            assert_eq!(listed(library, second, local).await, 2, "Personal still wins");
            sqlx::query("VACUUM").execute(&mut connection).await.unwrap();
            assert_eq!(listed(library, peer, local).await, 1);
            assert_eq!(listed(library, second, local).await, 2);

            let undone = library
                .hito
                .artist_catalog
                .undo_shared_artist_correction(
                    &operator,
                    &shared.correction_id,
                    UndoSharedArtistCorrection {
                        request_id: Uuid::new_v4(),
                        expected_revision: 1,
                    },
                )
                .await
                .unwrap();
            assert!(!undone.applied && undone.revision == 2);
            assert_eq!(listed(library, peer, local).await, 2);
            connection.close().await.unwrap();
        })
    })
    .await;
}

#[tokio::test]
async fn erasing_a_recording_removes_only_corrections_that_name_it() {
    run(|library| {
        Box::pin(async move {
            let people = populate(library).await;
            let (admin, peer, local, other) = (
                &library.actor,
                &people.peer,
                &library.local,
                &library.namesake,
            );
            link(library, admin, other, M2).await;
            choose(library, admin, other, M1).await;
            let mine = link(library, admin, local, M1).await;
            super::artist_correction_promotion::unlink(library, local, &mine.id).await;
            library
                .hito
                .artist_catalog
                .promote_artist_correction(admin, promote(0, unlinked(local, &mine.id)))
                .await
                .unwrap();
            let mut connection = connect(library).await;
            for recording in [local, other] {
                let observation: String = sqlx::query_scalar(
                    "SELECT metadata_observation_id FROM recordings WHERE id=?",
                )
                .bind(recording)
                .fetch_one(&mut connection)
                .await
                .unwrap();
                for statement in [
                    "INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin) VALUES(?1,?2,'USRC17607839','file_tag')",
                    "INSERT INTO recording_artist_url_observations(observation_id,recording_id,url) VALUES(?1,?2,'https://example.invalid/artist')",
                    "INSERT INTO recording_musicbrainz_observations(observation_id,entity,mbid,recording_id) VALUES(?1,'recording','11111111-1111-4111-8111-111111111111',?2)",
                ] {
                    sqlx::query(statement)
                        .bind(&observation)
                        .bind(recording)
                        .execute(&mut connection)
                        .await
                        .unwrap();
                }
            }
            let tables = [
                "recording_artist_associations",
                "recording_artist_association_changes",
                "recording_artist_association_change_order",
                "recording_artist_identity_choices",
                "shared_artist_correction_subjects",
                "recording_isrc_observations",
                "recording_artist_url_observations",
                "recording_musicbrainz_observations",
            ];
            let mut before = Vec::new();
            for table in tables {
                before.push((
                    count(
                        &mut connection,
                        &format!("SELECT count(*) FROM {table} WHERE recording_id=?"),
                        other,
                    )
                    .await,
                    count(
                        &mut connection,
                        &format!("SELECT count(*) FROM {table} WHERE recording_id=?"),
                        local,
                    )
                    .await,
                ));
            }
            assert!(before.iter().all(|(kept, erased)| *kept > 0 || *erased > 0));
            let receipts = owned(&mut connection, id(peer)).await;
            hito_core::test_support::correction_erasure::erase_recording_corrections(
                &mut connection,
                local,
            )
            .await
            .unwrap();
            for (table, (kept, _)) in tables.iter().zip(&before) {
                assert_eq!(
                    count(
                        &mut connection,
                        &format!("SELECT count(*) FROM {table} WHERE recording_id=?"),
                        local,
                    )
                    .await,
                    0,
                    "{table} keeps nothing about the erased recording"
                );
                assert_eq!(
                    count(
                        &mut connection,
                        &format!("SELECT count(*) FROM {table} WHERE recording_id=?"),
                        other,
                    )
                    .await,
                    *kept,
                    "{table} keeps the other recording"
                );
            }
            let after = owned(&mut connection, id(peer)).await;
            for (table, n) in &after {
                if table.contains("receipts") || table.contains("groups") {
                    assert!(receipts.contains(&(table.clone(), *n)));
                }
            }
            guard_is_empty(&mut connection).await;
            let exists: i64 = count(
                &mut connection,
                "SELECT count(*) FROM recordings WHERE id=?",
                local,
            )
            .await;
            assert_eq!(exists, 1, "The recording itself is not deleted here");
            connection.close().await.unwrap();
            assert_eq!(listed(library, peer, local).await, 0);
            assert_eq!(listed(library, admin, other).await, 2);
        })
    })
    .await;
}

#[tokio::test]
async fn a_failed_recording_erasure_on_a_plain_connection_leaves_no_scope_behind() {
    run(|library| {
        Box::pin(async move {
            let (admin, local) = (&library.actor, &library.local);
            link(library, admin, local, M1).await;
            let mut connection = connect(library).await;
            sqlx::query("CREATE TEMP TRIGGER refuse_association_delete BEFORE DELETE ON recording_artist_associations BEGIN SELECT RAISE(ABORT,'refused'); END")
                .execute(&mut connection)
                .await
                .unwrap();
            let failed = hito_core::test_support::correction_erasure::erase_recording_corrections(
                &mut connection,
                local,
            )
            .await;
            assert!(failed.is_err());
            guard_is_empty(&mut connection).await;
            let associations = count(
                &mut connection,
                "SELECT count(*) FROM recording_artist_associations WHERE recording_id=?",
                local,
            )
            .await;
            assert_eq!(associations, 1, "A failed erasure changes nothing");
            let direct = sqlx::query("DELETE FROM recording_artist_association_changes WHERE recording_id=?")
                .bind(local)
                .execute(&mut connection)
                .await;
            assert!(direct.is_err(), "A failed erasure does not unlock plain deletes");
            connection.close().await.unwrap();
        })
    })
    .await;
}
