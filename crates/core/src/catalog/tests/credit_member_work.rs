use crate::Search;
use crate::test_support::{support, work_meter::WorkMeter};

const RECORDINGS: i64 = 4000;
const PER_ROW_WORK: u64 = 1_000;
const FIXED_WORK: u64 = 60_000;

#[tokio::test]
async fn a_page_attaches_its_members_in_one_batch_whatever_the_library_holds() {
    let (_directory, hito, actor, library, _music) = support::library("credit-member-work").await;
    let pool = hito.catalog.store.pool.clone();
    {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<?2-1)
             INSERT INTO recordings(id,library_id,title,artist,duration_ms,initial_file_checksum,metadata_observation_id)
             SELECT 'r-'||substr('00000'||i,-5),?1,'Title '||i,'Ada & Bo',1000,'h-'||i,'o-'||i FROM n",
        )
        .bind(&library)
        .bind(RECORDINGS)
        .execute(&mut *connection)
        .await
        .unwrap();
    }
    let meter = WorkMeter::install(&pool).await;
    let (plain, plain_work) = meter
        .measure(async {
            hito.catalog
                .recordings(&actor, Search::default())
                .await
                .unwrap()
        })
        .await;
    {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&mut *connection)
            .await
            .unwrap();
        for (id, name) in [("artist-ada", "Ada"), ("artist-bo", "Bo")] {
            sqlx::query("INSERT INTO account_artists(user_id,id,library_id,name) VALUES(?,?,?,?)")
                .bind(actor.id())
                .bind(id)
                .bind(&library)
                .bind(name)
                .execute(&mut *connection)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO account_recording_credit_members(user_id,recording_id,artist_id,role)
             SELECT ?1,r.id,a.id,'main' FROM recordings r CROSS JOIN account_artists a
             WHERE r.library_id=?2 AND a.user_id=?1 AND a.id IN ('artist-ada','artist-bo')",
        )
        .bind(actor.id())
        .bind(&library)
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role)
             SELECT r.metadata_observation_id,p.position,r.id,p.name,'main' FROM recordings r
             CROSS JOIN (SELECT 0 AS position,'Ada' AS name UNION ALL SELECT 1,'Bo') p WHERE r.library_id=?1",
        )
        .bind(&library)
        .execute(&mut *connection)
        .await
        .unwrap();
    }
    let (with_members, member_work) = meter
        .measure(async {
            hito.catalog
                .recordings(&actor, Search::default())
                .await
                .unwrap()
        })
        .await;
    meter.remove(&pool).await;
    let page = plain.items.len() as u64;
    eprintln!("Library page VM_steps plain={plain_work} with_members={member_work} rows={page}");
    assert_eq!(plain.total, RECORDINGS);
    assert_eq!(with_members.items.len(), plain.items.len());
    assert!(
        with_members
            .items
            .iter()
            .all(|row| row.credited_artists.as_ref().is_some_and(|m| m.len() == 2))
    );
    assert!(plain.items.iter().all(|row| row.credited_artists.is_none()));
    assert!(
        member_work < plain_work + page * PER_ROW_WORK + FIXED_WORK,
        "Attaching members used {member_work} VM steps against {plain_work} without them for {page} rows beside {RECORDINGS} recordings"
    );
}
