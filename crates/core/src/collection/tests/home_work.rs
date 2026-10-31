use crate::test_support::{
    support::{library, scan, synchronize, write_wav},
    work_meter::WorkMeter,
};
use crate::{Action, Search};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use uuid::Uuid;

#[tokio::test]
async fn home_work_stays_bounded_as_listening_history_and_the_library_grow() {
    let (_dir, hito, actor, library_id, music) = library("home-work").await;
    for (offset, title) in ["Near one", "Near two", "Near three"]
        .into_iter()
        .enumerate()
    {
        let path = music.join(format!("{title}.wav"));
        write_wav(&path, offset as i16);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(title.into());
        tag.set_artist("Near artist".into());
        tag.set_album("Near album".into());
        tag.insert_text(ItemKey::AlbumArtist, "Near artist".into());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
    }
    scan(&hito, &actor, &library_id).await;
    let first = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    synchronize(
        &hito,
        &actor,
        vec![Action::ProgressSet {
            recording_id: Uuid::parse_str(&first).unwrap(),
            position_ms: 10,
            speed: 1.0,
        }],
    )
    .await;
    let meter = WorkMeter::install(&hito.collection.store.pool).await;
    let (small, small_work) = meter.measure(hito.collection.home(&actor)).await;
    assert_eq!(small.unwrap().unheard[0].tracks.len(), 2);

    let mut tx = hito.collection.store.pool.begin().await.unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 20000)
         INSERT INTO releases(id, library_id, title, artist, identity_key)
         SELECT printf('c2000000-0000-4000-8000-%012x', i), ?, printf('Old album %06d', i), 'Old artist', printf('old-%d', i) FROM n",
    )
    .bind(&library_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 20000)
         INSERT INTO recordings(id, library_id, release_id, title, artist, duration_ms, provenance)
         SELECT printf('c3000000-0000-4000-8000-%012x', i), ?, printf('c2000000-0000-4000-8000-%012x', i),
                printf('Old track %06d', i), 'Old artist', 1000, 'file_tags' FROM n",
    )
    .bind(&library_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    crate::metadata::provision(&mut tx, actor.id(), &library_id)
        .await
        .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 20000)
         INSERT INTO listening_progress(user_id, recording_id, position_ms, updated_at)
         SELECT ?, printf('c3000000-0000-4000-8000-%012x', i), 5, 1 FROM n",
    )
    .bind(actor.id())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 20000)
         INSERT INTO likes(user_id, recording_id, liked)
         SELECT ?, printf('c3000000-0000-4000-8000-%012x', i), 1 FROM n",
    )
    .bind(actor.id())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (large, large_work) = meter.measure(hito.collection.home(&actor)).await;
    meter.remove(&hito.collection.store.pool).await;
    let large = large.unwrap();
    eprintln!("Home VM_steps~ small={small_work} large={large_work}");
    assert_eq!(large.recent_albums.len(), 12);
    assert_eq!(large.liked_count, 1_000);
    assert!(
        large_work < small_work * 3 + 100_000,
        "home used approximately {large_work} VM steps with 20,000 other albums, plays and likes (small library: {small_work})"
    );
    assert!(large_work < 150_000);
}
