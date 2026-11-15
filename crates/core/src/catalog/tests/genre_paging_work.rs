use crate::test_support::{hang_guard::hang_guard, support, work_meter::WorkMeter};
use crate::{
    Actor,
    catalog::{
        genre_tracks::{ANCHOR_TITLE_SQL, accessible_query, candidates_query},
        genres::GenrePosition,
    },
};
use lofty::prelude::{Accessor, TagExt};
use sqlx::{QueryBuilder, Sqlite, Transaction};
use std::time::Duration;

const PAGE_SIZE: i64 = 20;
const PAGE_TIMEOUT: Duration = Duration::from_secs(5);

type PlanNode = (i64, i64, i64, String);
type PlanResult = Result<Result<Vec<PlanNode>, sqlx::Error>, tokio::time::error::Elapsed>;
type Timed<T> = (
    Result<Result<T, sqlx::Error>, tokio::time::error::Elapsed>,
    u64,
);

#[derive(Clone, Debug, PartialEq, Eq)]
struct PageSnapshot {
    ids: Vec<String>,
    next_after: Option<String>,
}

struct PageWork {
    size: usize,
    tile_count: i64,
    expected_first: PageSnapshot,
    expected_late: PageSnapshot,
    actual_first: Result<Result<crate::GenreTracksPage, crate::Error>, tokio::time::error::Elapsed>,
    actual_late: Result<Result<crate::GenreTracksPage, crate::Error>, tokio::time::error::Elapsed>,
    first_steps: u64,
    late_steps: u64,
    playback_closed: bool,
    replicas_closed: bool,
    replication_closed: bool,
    pool_closed: bool,
    statements: Option<StatementWork>,
}

struct StatementWork {
    accessible_plan: PlanResult,
    first_plan: PlanResult,
    late_plan: Option<PlanResult>,
    anchor_plan: Option<PlanResult>,
    accessible: Timed<Option<String>>,
    first_candidates: Timed<Vec<(String, String)>>,
    late_candidates: Option<Timed<Vec<(String, String)>>>,
    anchor: Option<Timed<Option<String>>>,
    expected_first_candidate_ids: Vec<String>,
    expected_late_candidate_ids: Vec<String>,
    anchor_matches_fixture: bool,
    transaction_committed: bool,
}

async fn explain_query(
    transaction: &mut Transaction<'_, Sqlite>,
    mut query: QueryBuilder<'static, Sqlite>,
) -> Result<Vec<PlanNode>, sqlx::Error> {
    let statement = format!("EXPLAIN QUERY PLAN {}", query.sql());
    let arguments = query.build().take_arguments().unwrap().unwrap();
    sqlx::query_as_with(&statement, arguments)
        .fetch_all(&mut **transaction)
        .await
}

fn snapshot(page: &crate::GenreTracksPage) -> PageSnapshot {
    PageSnapshot {
        ids: page
            .items
            .iter()
            .map(|recording| recording.id.clone())
            .collect(),
        next_after: page.next_after.clone(),
    }
}

fn expected_recording_order(size: usize) -> Vec<String> {
    let mut recordings: Vec<_> = (1..=size)
        .map(|index| {
            let title = match index % 10 {
                1 => "TIE".to_owned(),
                2 => "tie".to_owned(),
                value => format!("Song {value:02}"),
            };
            (title, format!("00000000-0000-4000-8000-{index:012x}"))
        })
        .collect();
    recordings.sort_by(|(left_title, left_id), (right_title, right_id)| {
        left_title
            .to_lowercase()
            .cmp(&right_title.to_lowercase())
            .then_with(|| left_id.cmp(right_id))
    });
    recordings.into_iter().map(|(_, id)| id).collect()
}

async fn measure_file_tag_population(size: usize, host: &str) -> PageWork {
    let (directory, hito, actor, library, music) =
        hang_guard("genre page work library", Box::pin(support::library(host))).await;
    hang_guard(
        "genre page work tagged recordings",
        Box::pin(async {
            let seed = music.join("folder-seed.wav");
            support::write_wav(&seed, 17);
            let mut tag = lofty::tag::Tag::new(lofty::tag::TagType::Id3v2);
            tag.set_title("Folder seed".to_owned());
            tag.set_artist("Genre page work artist".into());
            tag.set_genre("Jazz".to_owned());
            tag.save_to_path(&seed, Default::default()).unwrap();
            support::scan(&hito, &actor, &library).await;

            let folder_id: String = sqlx::query_scalar(
                "SELECT id FROM music_folders WHERE library_id = ? AND host_device = ? AND root_path = ? AND connected = 1 AND bound = 1",
            )
            .bind(&library)
            .bind(host)
            .bind(music.to_string_lossy().as_ref())
            .fetch_one(&hito.catalog.store.pool)
            .await
            .unwrap();

            for index in 1..=size {
                support::write_wav(&music.join(format!("page-{index:03}.wav")), index as i16 + 1);
            }

            let mut tx = hito.catalog.store.pool.begin().await.unwrap();
            sqlx::query(
                "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
                 INSERT INTO recordings(id, library_id, title, artist, duration_ms, initial_file_checksum)
                 SELECT printf('00000000-0000-4000-8000-%012x', i), ?1,
                        CASE i % 10 WHEN 1 THEN 'TIE' WHEN 2 THEN 'tie' ELSE printf('Song %02d', i % 10) END,
                        'Genre page work artist', 1000, 'genre-page-work-checksum-' || i
                 FROM n",
            )
            .bind(&library)
            .bind(size as i64)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
                 INSERT INTO sources(id, recording_id, provider, provider_id, device_id, ready)
                 SELECT 'genre-page-work-source-' || i,
                        printf('00000000-0000-4000-8000-%012x', i), 'local',
                        'genre-page-work-provider-' || i, ?2, 1
                 FROM n",
            )
            .bind(size as i64)
            .bind(host)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
                 INSERT INTO local_files(source_id, library_id, folder_id, relative_path, path, scan_root, size_bytes)
                 SELECT 'genre-page-work-source-' || i, ?2, ?3,
                        json_quote('page-' || printf('%03d', i) || '.wav'),
                        ?4 || '/page-' || printf('%03d', i) || '.wav', ?4, 16044
                 FROM n",
            )
            .bind(size as i64)
            .bind(&library)
            .bind(&folder_id)
            .bind(music.to_string_lossy().as_ref())
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
                 INSERT INTO source_genres(source_id, genre_key, label)
                 SELECT 'genre-page-work-source-' || i, 'fado', 'Fado' FROM n",
            )
            .bind(size as i64)
            .execute(&mut *tx)
            .await
            .unwrap();
            tx.commit().await.unwrap();
        }),
    )
    .await;

    let genres = tokio::time::timeout(PAGE_TIMEOUT, hito.catalog.genres(&actor))
        .await
        .expect("genre page work tiles finish within 5 seconds")
        .unwrap();
    let tile_count = genres
        .iter()
        .find(|genre| genre.key == "fado")
        .map(|genre| genre.track_count)
        .unwrap_or_default();
    assert_eq!(tile_count, size as i64);

    let ordered_ids = expected_recording_order(size);
    let page_count = size.div_ceil(PAGE_SIZE as usize);
    let mut expected_pages = Vec::with_capacity(page_count);
    let mut after = None;
    let mut late_anchor = None;
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_cursors = std::collections::HashSet::new();
    for page_number in 0..page_count {
        let page = tokio::time::timeout(
            PAGE_TIMEOUT,
            hito.catalog
                .genre_tracks(&actor, "Fado", Some(PAGE_SIZE), after.as_deref()),
        )
        .await
        .expect("genre page work baseline finishes within 5 seconds")
        .unwrap();
        let expected_items = (size - page_number * PAGE_SIZE as usize).min(PAGE_SIZE as usize);
        assert_eq!(page.items.len(), expected_items);
        let current = snapshot(&page);
        let start = page_number * PAGE_SIZE as usize;
        let end = (start + expected_items).min(size);
        assert_eq!(current.ids, ordered_ids[start..end].to_vec());
        assert!(!current.ids.is_empty());
        assert!(current.ids.iter().all(|id| seen_ids.insert(id.clone())));
        assert_eq!(current.next_after.is_some(), page_number + 1 < page_count);
        if let Some(cursor) = current.next_after.as_ref() {
            assert!(seen_cursors.insert(cursor.clone()));
        }
        after = current.next_after.clone();
        if page_number + 1 == page_count - 1 {
            late_anchor = page
                .items
                .last()
                .map(|recording| (recording.id.to_string(), recording.title.clone()));
        }
        expected_pages.push(current);
    }
    assert_eq!(seen_ids.len(), size);
    let late_after = expected_pages[page_count - 2]
        .next_after
        .clone()
        .expect("late page cursor comes from the public page walk");
    let expected_first = expected_pages[0].clone();
    let expected_late = expected_pages[page_count - 1].clone();

    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let (actual_first, first_steps) = meter
        .measure(async {
            tokio::time::timeout(
                PAGE_TIMEOUT,
                hito.catalog
                    .genre_tracks(&actor, "Fado", Some(PAGE_SIZE), None),
            )
            .await
        })
        .await;
    let (actual_late, late_steps) = meter
        .measure(async {
            tokio::time::timeout(
                PAGE_TIMEOUT,
                hito.catalog
                    .genre_tracks(&actor, "Fado", Some(PAGE_SIZE), Some(&late_after)),
            )
            .await
        })
        .await;

    let mut statements = None;
    if let Ok(Ok(mut transaction)) =
        tokio::time::timeout(PAGE_TIMEOUT, hito.catalog.store.pool.begin()).await
    {
        let accessible_plan = tokio::time::timeout(
            PAGE_TIMEOUT,
            explain_query(
                &mut transaction,
                accessible_query(&hito.catalog.host, &actor, "fado"),
            ),
        )
        .await;
        let first_plan = tokio::time::timeout(
            PAGE_TIMEOUT,
            explain_query(
                &mut transaction,
                candidates_query(&hito.catalog.host, &actor, "fado", PAGE_SIZE, None),
            ),
        )
        .await;
        let (accessible, accessible_steps) = meter
            .measure(tokio::time::timeout(PAGE_TIMEOUT, async {
                accessible_query(&hito.catalog.host, &actor, "fado")
                    .build_query_scalar::<String>()
                    .fetch_optional(&mut *transaction)
                    .await
            }))
            .await;
        let (first_candidates, first_candidate_steps) = meter
            .measure(tokio::time::timeout(PAGE_TIMEOUT, async {
                candidates_query(&hito.catalog.host, &actor, "fado", PAGE_SIZE, None)
                    .build_query_as::<(String, String)>()
                    .fetch_all(&mut *transaction)
                    .await
            }))
            .await;

        let mut anchor_plan = None;
        let mut anchor = None;
        let mut late_plan = None;
        let mut late_candidates = None;
        let mut anchor_matches_fixture = false;
        if let Some((anchor_id, expected_title)) = late_anchor.as_ref() {
            let mut anchor_builder = QueryBuilder::<Sqlite>::new(ANCHOR_TITLE_SQL);
            anchor_builder
                .push_bind(actor.id().to_owned())
                .push_bind(anchor_id.clone());
            anchor_plan = Some(
                tokio::time::timeout(
                    PAGE_TIMEOUT,
                    explain_query(&mut transaction, anchor_builder),
                )
                .await,
            );
            let (result, steps) = meter
                .measure(tokio::time::timeout(PAGE_TIMEOUT, async {
                    sqlx::query_scalar::<_, String>(ANCHOR_TITLE_SQL)
                        .bind(actor.id().to_owned())
                        .bind(anchor_id.clone())
                        .fetch_optional(&mut *transaction)
                        .await
                }))
                .await;
            anchor_matches_fixture = result
                .as_ref()
                .ok()
                .and_then(|result| result.as_ref().ok())
                .and_then(Option::as_deref)
                == Some(expected_title.as_str());
            let late_title = result
                .as_ref()
                .ok()
                .and_then(|result| result.as_ref().ok())
                .and_then(Option::as_deref)
                .map(str::to_owned);
            anchor = Some((result, steps));
            if let Some(title) = late_title {
                let after = GenrePosition {
                    title: &title,
                    id: anchor_id,
                };
                late_plan = Some(
                    tokio::time::timeout(
                        PAGE_TIMEOUT,
                        explain_query(
                            &mut transaction,
                            candidates_query(
                                &hito.catalog.host,
                                &actor,
                                "fado",
                                PAGE_SIZE,
                                Some(after),
                            ),
                        ),
                    )
                    .await,
                );
                late_candidates = Some(
                    meter
                        .measure(tokio::time::timeout(PAGE_TIMEOUT, async {
                            candidates_query(
                                &hito.catalog.host,
                                &actor,
                                "fado",
                                PAGE_SIZE,
                                Some(after),
                            )
                            .build_query_as::<(String, String)>()
                            .fetch_all(&mut *transaction)
                            .await
                        }))
                        .await,
                );
            }
        }
        let transaction_committed = tokio::time::timeout(PAGE_TIMEOUT, transaction.commit())
            .await
            .is_ok_and(|result| result.is_ok());
        statements = Some(StatementWork {
            accessible_plan,
            first_plan,
            late_plan,
            anchor_plan,
            accessible: (accessible, accessible_steps),
            first_candidates: (first_candidates, first_candidate_steps),
            late_candidates,
            anchor,
            expected_first_candidate_ids: ordered_ids
                .iter()
                .take(PAGE_SIZE as usize + 1)
                .cloned()
                .collect(),
            expected_late_candidate_ids: ordered_ids
                .iter()
                .skip(size - PAGE_SIZE as usize)
                .cloned()
                .collect(),
            anchor_matches_fixture,
            transaction_committed,
        });
    }
    meter.remove(&hito.catalog.store.pool).await;

    let playback_closed = tokio::time::timeout(PAGE_TIMEOUT, hito.playback.close())
        .await
        .is_ok_and(|result| result.is_ok());
    let replicas_closed = tokio::time::timeout(PAGE_TIMEOUT, hito.replicas.close())
        .await
        .is_ok_and(|result| result.is_ok());
    let replication_closed = tokio::time::timeout(PAGE_TIMEOUT, hito.catalog_replication.close())
        .await
        .is_ok_and(|result| result.is_ok());
    let pool_closed = tokio::time::timeout(PAGE_TIMEOUT, hito.catalog.store.pool.close())
        .await
        .is_ok();
    drop(hito);
    drop(directory);

    PageWork {
        size,
        tile_count,
        expected_first,
        expected_late,
        actual_first,
        actual_late,
        first_steps,
        late_steps,
        playback_closed,
        replicas_closed,
        replication_closed,
        pool_closed,
        statements,
    }
}

#[tokio::test]
async fn file_tag_genre_pages_measure_first_and_late_population_work() {
    let small = measure_file_tag_population(40, "genre-page-work-40").await;
    let large = measure_file_tag_population(300, "genre-page-work-300").await;

    for result in [&small, &large] {
        assert!(result.playback_closed);
        assert!(result.replicas_closed);
        assert!(result.replication_closed);
        assert!(result.pool_closed);
        assert_eq!(result.tile_count, result.size as i64);
        let first = result
            .actual_first
            .as_ref()
            .expect("first genre page finishes within 5 seconds")
            .as_ref()
            .expect("first genre page succeeds");
        let late = result
            .actual_late
            .as_ref()
            .expect("late genre page finishes within 5 seconds")
            .as_ref()
            .expect("late genre page succeeds");
        assert_eq!(snapshot(first), result.expected_first);
        assert_eq!(snapshot(late), result.expected_late);
        assert_eq!(first.items.len(), PAGE_SIZE as usize);
        assert_eq!(late.items.len(), PAGE_SIZE as usize);
        assert!(first.next_after.is_some());
        assert!(late.next_after.is_none());
        assert!(result.first_steps > 0);
        assert!(result.late_steps > 0);
        let statements = result
            .statements
            .as_ref()
            .expect("genre paging diagnostic transaction starts");
        assert!(statements.transaction_committed);
        assert!(statements
            .accessible_plan
            .as_ref()
            .is_ok_and(|plan| plan.is_ok()));
        assert!(statements
            .first_plan
            .as_ref()
            .is_ok_and(|plan| plan.is_ok()));
        assert!(statements
            .accessible
            .0
            .as_ref()
            .is_ok_and(|query| query.as_ref().is_ok_and(Option::is_some)));
        assert!(statements
            .first_candidates
            .0
            .as_ref()
            .is_ok_and(|query| query.as_ref().is_ok_and(|rows| rows.len() == PAGE_SIZE as usize + 1)));
        assert!(statements.first_candidates.1 > 0);
        assert!(statements.anchor_matches_fixture);
        let anchor_plan = statements
            .anchor_plan
            .as_ref()
            .expect("late public cursor resolves its anchor plan");
        assert!(anchor_plan.as_ref().is_ok_and(|plan| plan.is_ok()));
        let late_plan = statements
            .late_plan
            .as_ref()
            .expect("late public cursor produces a candidate plan");
        assert!(late_plan.as_ref().is_ok_and(|plan| plan.is_ok()));
        let (anchor, anchor_steps) = statements
            .anchor
            .as_ref()
            .expect("late public cursor resolves its anchor");
        assert!(anchor
            .as_ref()
            .is_ok_and(|query| query.as_ref().is_ok_and(Option::is_some)));
        assert!(*anchor_steps > 0);
        let (late_candidates, late_candidate_steps) = statements
            .late_candidates
            .as_ref()
            .expect("late public cursor measures candidate work");
        assert!(late_candidates.as_ref().is_ok_and(|query| query
            .as_ref()
            .is_ok_and(|rows| rows.len() == result.expected_late.ids.len())));
        assert!(*late_candidate_steps > 0);
        assert_eq!(
            statements
                .first_candidates
                .0
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            statements.expected_first_candidate_ids
        );
        assert_eq!(
            late_candidates
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            statements.expected_late_candidate_ids
        );
        let (accessible, accessible_steps) = &statements.accessible;
        assert!(accessible
            .as_ref()
            .is_ok_and(|query| query.as_ref().is_ok_and(Option::is_some)));
        assert!(*accessible_steps > 0);
        let first_eqp = statements.first_plan.as_ref().unwrap().as_ref().unwrap();
        let accessible_eqp = statements
            .accessible_plan
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        let anchor_eqp = anchor_plan.as_ref().unwrap().as_ref().unwrap();
        let late_eqp = late_plan.as_ref().unwrap().as_ref().unwrap();
        assert!(!accessible_eqp.is_empty());
        assert!(!first_eqp.is_empty());
        assert!(!anchor_eqp.is_empty());
        assert!(!late_eqp.is_empty());
        let accessible_rows = if accessible.as_ref().unwrap().as_ref().unwrap().is_some() {
            1
        } else {
            0
        };
        eprintln!(
            "genre_page_statement case=file-tag size={} statement=accessible vm_steps={} row_count={}",
            result.size,
            accessible_steps,
            accessible_rows
        );
        eprintln!(
            "genre_page_statement case=file-tag size={} statement=candidates-first vm_steps={} row_count={}",
            result.size,
            statements.first_candidates.1,
            statements.first_candidates.0.as_ref().unwrap().as_ref().unwrap().len()
        );
        eprintln!(
            "genre_page_statement case=file-tag size={} statement=anchor-late vm_steps={} row_count=1",
            result.size,
            anchor_steps
        );
        eprintln!(
            "genre_page_statement case=file-tag size={} statement=candidates-late vm_steps={} row_count={}",
            result.size,
            late_candidate_steps,
            late_candidates.as_ref().unwrap().as_ref().unwrap().len()
        );
        eprintln!(
            "genre_page_eqp case=file-tag size={} statement=accessible nodes={:?}",
            result.size,
            accessible_eqp
        );
        eprintln!(
            "genre_page_eqp case=file-tag size={} statement=candidates-first nodes={:?}",
            result.size,
            first_eqp
        );
        eprintln!(
            "genre_page_eqp case=file-tag size={} statement=anchor-late nodes={:?}",
            result.size,
            anchor_eqp
        );
        eprintln!(
            "genre_page_eqp case=file-tag size={} statement=candidates-late nodes={:?}",
            result.size,
            late_eqp
        );
        eprintln!(
            "genre_page_work case=file-tag size={} page=first vm_steps={} item_count={} next_after={}",
            result.size,
            result.first_steps,
            first.items.len(),
            first.next_after.is_some()
        );
        eprintln!(
            "genre_page_work case=file-tag size={} page=late vm_steps={} item_count={} next_after={}",
            result.size,
            result.late_steps,
            late.items.len(),
            late.next_after.is_some()
        );
    }
}
