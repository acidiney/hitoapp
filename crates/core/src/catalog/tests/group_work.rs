use crate::test_support::{support, work_meter::WorkMeter};
use crate::{Search, catalog::group_step};

const FIXED_WORK: u64 = 60_000;

async fn seed_strangers(pool: &sqlx::SqlitePool, count: i64) {
    let mut connection = pool.acquire().await.unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("INSERT OR IGNORE INTO libraries(id,name) VALUES('strangers','Strangers')")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<?1-1)
         INSERT INTO recordings(id,library_id,title,artist,duration_ms,initial_file_checksum,metadata_observation_id)
         SELECT 'stranger-'||i,'strangers','T','A',1000,'stranger-'||i,'observation-'||i FROM n",
    )
    .bind(count)
    .execute(&mut *connection)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO recording_isrc_observations(observation_id,recording_id,isrc,origin)
         SELECT metadata_observation_id,id,'GB'||substr('000'||(rowid%1000),-3)||'26'||substr('0000'||(rowid/1000),-5),'file_tag'
         FROM recordings WHERE library_id='strangers'",
    )
    .execute(&mut *connection)
    .await
    .unwrap();
}

async fn first_step_work(total: i64) -> u64 {
    let (_directory, hito, actor, _library, _music) = support::library("group-work").await;
    let pool = hito.catalog.store.pool.clone();
    seed_strangers(&pool, total).await;
    let meter = WorkMeter::install(&pool).await;
    let (step, work) = meter
        .measure(async {
            group_step(&hito.catalog.store, actor.id(), Some("i:"))
                .await
                .unwrap()
        })
        .await;
    meter.remove(&pool).await;
    assert!(step.next.is_some(), "{total} rows need more than one page");
    assert_eq!(step.grouped, 0);
    work
}

#[tokio::test]
async fn a_grouping_step_reads_a_bounded_page_whatever_other_libraries_hold() {
    let small = first_step_work(800).await;
    let large = first_step_work(8000).await;
    eprintln!("Recording grouping step VM_steps small={small} large={large}");
    let budget = small + FIXED_WORK;
    assert!(
        large < budget,
        "A step used {large} VM steps beside 8000 foreign identifiers after {small} beside 800; budget {budget}"
    );
}

#[tokio::test]
async fn the_library_list_probes_group_membership_by_key() {
    let (_directory, hito, actor, library, _music) = support::library("group-list-work").await;
    let pool = hito.catalog.store.pool.clone();
    {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<3999)
             INSERT INTO recordings(id,library_id,title,artist,duration_ms,initial_file_checksum)
             SELECT 'r-'||substr('00000'||i,-5),?1,'Title '||i,'A',1000,'h-'||i FROM n",
        )
        .bind(&library)
        .execute(&mut *connection)
        .await
        .unwrap();
    }
    let meter = WorkMeter::install(&pool).await;
    let read = async || {
        meter
            .measure(async {
                hito.catalog
                    .recordings(&actor, Search::default())
                    .await
                    .unwrap()
            })
            .await
    };
    let (alone, plain) = read().await;
    {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO recording_group_members(user_id,recording_id,canonical_id,proof)
             SELECT ?1,'r-'||substr('00000'||(i+2000),-5),'r-'||substr('00000'||i,-5),'isrc'
             FROM (WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<1999) SELECT i FROM n)",
        )
        .bind(actor.id())
        .execute(&mut *connection)
        .await
        .unwrap();
    }
    let (grouped, with_groups) = read().await;
    meter.remove(&pool).await;
    eprintln!("Library list VM_steps ungrouped={plain} grouped={with_groups}");
    assert_eq!(alone.total, 4000);
    assert_eq!(grouped.total, 2000);
    assert!(
        with_groups < plain + 2000 * 200 + FIXED_WORK,
        "Deciding 2000 grouped copies used {with_groups} VM steps against {plain} without groups"
    );
}
