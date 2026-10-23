use crate::test_support::work_meter::WorkMeter;
use crate::{
    Action, Actor, AudiobookChapterAnchor, CoreConfig, Credentials, Error, HostDeviceId, Operation,
    Services, SyncBatch, open,
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn chapter_admission_work_depends_on_requested_anchors_under_all_statistics() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let config = || {
        CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("chapter-admission-work").unwrap(),
        )
    };
    let hito = open(config()).await.unwrap();
    let admin_session = hito
        .identity
        .setup(credentials("chapter-work-admin"))
        .await
        .unwrap();
    let admin = hito
        .identity
        .authenticate(&admin_session.token)
        .await
        .unwrap();
    let library = hito.catalog.libraries(&admin).await.unwrap().remove(0).id;
    let owner_user = hito
        .identity
        .create_user(&admin, credentials("chapter-work-owner"))
        .await
        .unwrap();
    let peer_user = hito
        .identity
        .create_user(&admin, credentials("chapter-work-peer"))
        .await
        .unwrap();
    for user in [&owner_user, &peer_user] {
        hito.identity
            .set_library_access(&admin, &library, &user.id, true)
            .await
            .unwrap();
    }
    let owner_session = hito
        .identity
        .login(credentials("chapter-work-owner"))
        .await
        .unwrap();
    let peer_session = hito
        .identity
        .login(credentials("chapter-work-peer"))
        .await
        .unwrap();
    let owner = hito
        .identity
        .authenticate(&owner_session.token)
        .await
        .unwrap();
    let peer = hito
        .identity
        .authenticate(&peer_session.token)
        .await
        .unwrap();
    assert!(!owner.user().admin && !peer.user().admin);
    insert_recordings(&hito, &library, 0, 1).await;
    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    for actor in [&owner, &peer] {
        crate::metadata::provision(&mut tx, actor.user().id.as_str(), &library)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    let book = Uuid::new_v4();
    let foreign_book = Uuid::new_v4();
    for (actor, id, position) in [(&owner, book, 111), (&peer, foreign_book, 555)] {
        accept(
            &hito,
            actor,
            vec![
                Action::PlaylistCreate {
                    playlist_id: id,
                    name: "Owned work audiobook".into(),
                },
                Action::EntryInsert {
                    playlist_id: id,
                    entry_id: chapter_id(0),
                    recording_id: recording_id(0),
                    position: 0,
                },
                Action::AudiobookCreate { playlist_id: id },
                Action::ProgressSet {
                    recording_id: recording_id(0),
                    position_ms: position,
                    speed: 1.25,
                },
                Action::AudiobookChapterSet {
                    chapter: anchor(id, 0),
                },
            ],
        )
        .await;
    }
    sqlx::query("ANALYZE")
        .execute(&hito.collection.store.pool)
        .await
        .unwrap();
    let inherited: String =
        sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='recordings_library'")
            .fetch_one(&hito.collection.store.pool)
            .await
            .unwrap();
    assert_eq!(inherited.split_whitespace().next(), Some("1"));
    insert_recordings(&hito, &library, 1, 100_000).await;
    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    let chapters = sqlx::query(
        "WITH RECURSIVE fixture(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM fixture WHERE i<9999)
         INSERT INTO playlist_entries(playlist_id,id,recording_id,position)
         SELECT ?,printf('ad200000-0000-4000-8000-%012x',i),
             printf('ad100000-0000-4000-8000-%012x',i*9),i FROM fixture",
    )
    .bind(book.to_string())
    .execute(&mut *tx)
    .await
    .unwrap();
    assert_eq!(chapters.rows_affected(), 9_999);
    sqlx::query(
        "WITH RECURSIVE fixture(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM fixture WHERE i<999)
         INSERT INTO playlists(id,user_id,name)
         SELECT printf('ad300000-0000-4000-8000-%012x',i),?,'Unrelated work audiobook' FROM fixture",
    ).bind(peer.user().id.clone()).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO audiobooks(playlist_id,user_id) SELECT id,user_id FROM playlists
         WHERE id LIKE 'ad300000-%'",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO playlist_entries(playlist_id,id,recording_id,position)
         SELECT id,'ad400000-0000-4000-8000-000000000000','ad100000-0000-4000-8000-000000000000',0 FROM playlists
         WHERE id LIKE 'ad300000-%'",
    ).execute(&mut *tx).await.unwrap();
    crate::metadata::provision(&mut tx, owner.user().id.as_str(), &library)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM recordings),(SELECT count(*) FROM playlist_entries WHERE playlist_id=?),
            (SELECT count(*) FROM audiobooks),(SELECT count(*) FROM account_recording_credits WHERE user_id=?)",
    ).bind(book.to_string()).bind(owner.user().id.clone()).fetch_one(&hito.collection.store.pool).await.unwrap();
    assert_eq!(counts, (100_000, 10_000, 1002, 100_000));
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&hito.collection.store.pool)
            .await
            .unwrap()
            .is_empty()
    );
    let retained: String =
        sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='recordings_library'")
            .fetch_one(&hito.collection.store.pool)
            .await
            .unwrap();
    assert_eq!(retained, inherited);
    hito.collection.store.pool.close().await;
    drop(hito);

    for statistics in ["inherited", "absent", "fresh"] {
        let hito = open(config()).await.unwrap();
        match statistics {
            "absent" => {
                sqlx::raw_sql("DELETE FROM sqlite_stat1; DELETE FROM sqlite_stat4;")
                    .execute(&hito.collection.store.pool)
                    .await
                    .unwrap();
            }
            "fresh" => {
                sqlx::query("ANALYZE")
                    .execute(&hito.collection.store.pool)
                    .await
                    .unwrap();
            }
            _ => {}
        }
        hito.collection.store.pool.close().await;
        drop(hito);
        let hito = open(config()).await.unwrap();
        let owner = hito
            .identity
            .authenticate(&owner_session.token)
            .await
            .unwrap();
        let peer = hito
            .identity
            .authenticate(&peer_session.token)
            .await
            .unwrap();
        let stat: Option<String> =
            sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='recordings_library'")
                .fetch_optional(&hito.collection.store.pool)
                .await
                .unwrap();
        assert_eq!(
            stat.as_deref()
                .and_then(|value| value.split_whitespace().next()),
            match statistics {
                "inherited" => Some("1"),
                "fresh" => Some("100000"),
                _ => None,
            }
        );
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&hito.collection.store.pool)
            .await
            .unwrap();
        let stat4: bool = sqlx::query_scalar("SELECT sqlite_compileoption_used('ENABLE_STAT4')")
            .fetch_one(&hito.collection.store.pool)
            .await
            .unwrap();
        let before = private_state(&hito, &owner, &peer, book, foreign_book).await;
        let meter = WorkMeter::install(&hito.collection.store.pool).await;
        let exercise = AssertUnwindSafe(async {
            for requested in [vec![anchor(book, 9_999)], (0..1000).map(|index| anchor(book, index * 10)).collect(), vec![anchor(book, 9_999); 1000]] {
                let (result, work) = meter.measure(hito.collection.check_audiobook_chapters(&owner, &requested)).await;
                eprintln!("Chapter admission public VM_steps~ sqlite={version} stat4={stat4} statistics={statistics} records=100000 chapters=10000 requested={} work={work} outcome={:?}", requested.len(), result.as_ref().map(|_| "accepted").map_err(Error::code));
                assert!(result.is_ok(), "The actual public capability must admit every owned requested occurrence");
                assert!(work < 5_000 + 250 * requested.len() as u64, "Chapter admission exceeded its fixed point-lookup work budget");
                if requested.len() == 1000 { assert!(work > 0); }
            }
            for requested in [vec![anchor(foreign_book, 0)], vec![anchor(book, 9_999), anchor(foreign_book, 0)]] {
                let (result, work) = meter.measure(hito.collection.check_audiobook_chapters(&owner, &requested)).await;
                eprintln!("Chapter admission denied VM_steps~ sqlite={version} stat4={stat4} statistics={statistics} requested={} work={work} outcome={:?}", requested.len(), result.as_ref().map(|_| "accepted").map_err(Error::code));
                assert!(matches!(result, Err(Error::NotFound)));
                assert!(work < 5_000 + 250 * requested.len() as u64, "Private chapter admission exceeded its fixed point-lookup work budget");
            }
        }).catch_unwind().await;
        meter.remove(&hito.collection.store.pool).await;
        if let Err(panic) = exercise {
            hito.collection.store.pool.close().await;
            std::panic::resume_unwind(panic);
        }
        assert_eq!(
            private_state(&hito, &owner, &peer, book, foreign_book).await,
            before
        );
        hito.collection.store.pool.close().await;
    }
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "owned chapter work password".into(),
    }
}

fn recording_id(index: usize) -> Uuid {
    Uuid::parse_str(&format!("ad100000-0000-4000-8000-{index:012x}")).unwrap()
}

fn chapter_id(index: usize) -> Uuid {
    Uuid::parse_str(&format!("ad200000-0000-4000-8000-{index:012x}")).unwrap()
}

fn anchor(book: Uuid, index: usize) -> AudiobookChapterAnchor {
    AudiobookChapterAnchor {
        audiobook_id: book,
        entry_id: chapter_id(index),
        recording_id: recording_id(index * 9),
    }
}

async fn accept(hito: &Services, actor: &Actor, actions: Vec<Action>) {
    let operations = actions
        .into_iter()
        .map(|action| Operation {
            id: Uuid::new_v4(),
            action,
        })
        .collect::<Vec<_>>();
    let ids = operations
        .iter()
        .map(|operation| operation.id)
        .collect::<Vec<_>>();
    let receipt = hito
        .collection
        .apply(
            actor,
            SyncBatch {
                device_id: "chapter-admission-owned-work".into(),
                operations,
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.acknowledged, ids);
}

async fn insert_recordings(hito: &Services, library: &str, first: usize, end: usize) {
    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    let inserted = sqlx::query(
        "WITH RECURSIVE fixture(i) AS (VALUES(?) UNION ALL SELECT i+1 FROM fixture WHERE i+1<?)
         INSERT INTO recordings(id,library_id,title,artist,duration_ms,provenance)
         SELECT printf('ad100000-0000-4000-8000-%012x',i),?,'Owned work recording','Owned work narrator',1000,'file_tags' FROM fixture",
    ).bind(first as i64).bind(end as i64).bind(library).execute(&mut *tx).await.unwrap();
    assert_eq!(inserted.rows_affected(), (end - first) as u64);
    tx.commit().await.unwrap();
}

async fn private_state(
    hito: &Services,
    owner: &Actor,
    peer: &Actor,
    book: Uuid,
    foreign: Uuid,
) -> Value {
    json!({
        "book": hito.collection.audiobook(owner, &book.to_string()).await.unwrap(),
        "owner_progress": hito.collection.saved_playback(owner, &[recording_id(0)]).await.unwrap(),
        "owner_events": hito.collection.events(owner, 0).await.unwrap(),
        "peer_book": hito.collection.audiobook(peer, &foreign.to_string()).await.unwrap(),
        "peer_progress": hito.collection.saved_playback(peer, &[recording_id(0)]).await.unwrap(),
        "peer_events": hito.collection.events(peer, 0).await.unwrap()
    })
}
