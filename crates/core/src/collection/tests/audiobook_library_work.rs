use crate::test_support::work_meter::WorkMeter;
use crate::{AudiobookLibraryQuery, CoreConfig, Credentials, HostDeviceId, open};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;

const PAGE_WORK_BUDGET: u64 = 50_000;

#[tokio::test]
async fn audiobook_pages_stay_bounded_with_a_large_unrelated_playlist_library() {
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("music");
    std::fs::create_dir(&music).unwrap();
    let config = || {
        CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("audiobook-list-work").unwrap(),
        )
    };
    let hito = open(config()).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "owned-list-work".into(),
            password: "owned audiobook listing password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    sqlx::query("INSERT INTO playlists(id,user_id,name) VALUES('b1000000-0000-4000-8000-000000000001',?,'Z owned work book')")
        .bind(actor.id()).execute(&hito.collection.store.pool).await.unwrap();
    sqlx::query("INSERT INTO audiobooks(playlist_id,user_id) SELECT id,user_id FROM playlists")
        .execute(&hito.collection.store.pool)
        .await
        .unwrap();
    sqlx::query("ANALYZE")
        .execute(&hito.collection.store.pool)
        .await
        .unwrap();
    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    sqlx::query("WITH RECURSIVE sequence(n) AS (VALUES(2) UNION ALL SELECT n+1 FROM sequence WHERE n<100000)
        INSERT INTO playlists(id,user_id,name)
        SELECT printf('b1000000-0000-4000-8000-%012x',n),?,CASE WHEN n>99601 THEN 'Z owned work book' ELSE printf('A unrelated selection %06d',n) END FROM sequence")
        .bind(actor.id()).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO audiobooks(playlist_id,user_id)
        SELECT id,user_id FROM playlists WHERE user_id=? ORDER BY id DESC LIMIT 399",
    )
    .bind(actor.id())
    .execute(&mut *tx)
    .await
    .unwrap();
    let counts: (i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM playlists),(SELECT count(*) FROM audiobooks)")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(counts, (100_000, 400));
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *tx)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
    hito.collection.store.pool.close().await;
    drop(hito);
    let mut measurements = Vec::new();
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
        let actor = hito.identity.authenticate(&session.token).await.unwrap();
        let stat: Option<String> =
            sqlx::query_scalar("SELECT stat FROM sqlite_stat1 WHERE idx='audiobooks_account'")
                .fetch_optional(&hito.collection.store.pool)
                .await
                .unwrap();
        assert_eq!(
            stat.as_deref()
                .and_then(|stat| stat.split_whitespace().next()),
            match statistics {
                "inherited" => Some("1"),
                "fresh" => Some("400"),
                _ => None,
            }
        );
        let meter = WorkMeter::install(&hito.collection.store.pool).await;
        let exercise = AssertUnwindSafe(async {
            let mut cursor = None;
            let mut ids = Vec::new();
            loop {
                let (page,work) = meter.measure(hito.collection.audiobook_library(&actor, AudiobookLibraryQuery { limit: Some(100), cursor })).await;
                eprintln!("Audiobook public page completed statistics={statistics} depth={} work={work} outcome={}", ids.len(), page.as_ref().map(|_| "read").unwrap_or_else(|error| error.code()));
                assert!(work>0 && work<PAGE_WORK_BUDGET,"Audiobook public page exceeded its {PAGE_WORK_BUDGET} VM-step budget with {statistics} statistics at depth {}: {work}",ids.len());
                let page = page.unwrap();
                assert_eq!(page.items.len(),100);
                assert_eq!(page.pagination.limit,100);
                eprintln!("Audiobook public page VM_steps~ statistics={statistics} playlists=100000 grouped=400 depth={} work={work}",ids.len());
                measurements.push((statistics,ids.len(),work));
                ids.extend(page.items.into_iter().map(|book| book.id));
                cursor = page.pagination.next_cursor;
                if cursor.is_none() { break; }
                assert!(ids.len()<400,"An indexed public continuation must finish without repeating pages");
            }
            let expected = std::iter::once(1).chain(99_602..=100_000)
                .map(|index| format!("b1000000-0000-4000-8000-{index:012x}")).collect::<Vec<_>>();
            assert_eq!(ids,expected,"Equal names retain exact ID order across every page");
        }).catch_unwind().await;
        meter.remove(&hito.collection.store.pool).await;
        hito.collection.store.pool.close().await;
        if let Err(panic) = exercise {
            std::panic::resume_unwind(panic);
        }
    }
    for (statistics, depth, work) in measurements {
        assert!(
            work > 0 && work < PAGE_WORK_BUDGET,
            "Audiobook public page at {depth} with {statistics} statistics used {work} VM steps; budget={PAGE_WORK_BUDGET}"
        );
    }
}
