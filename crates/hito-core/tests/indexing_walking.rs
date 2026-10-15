mod support;

use hito_core::Search;

#[tokio::test]
async fn excessive_directory_depth_fails_without_hiding_previously_verified_audio() {
    let (_dir, hito, actor, library, music) = support::library("server").await;
    support::write_wav(&music.join("track.wav"), 0);
    support::scan(&hito, &actor, &library).await;
    let mut folder = music;
    for _ in 0..65 {
        folder = folder.join("nested");
        std::fs::create_dir(&folder).unwrap();
    }
    std::fs::write(folder.join("unrelated.txt"), "Not audio").unwrap();
    let job = hito
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(hito.storage.run_next_job().await.unwrap());
    assert_eq!(
        hito.storage.job(&actor, &job.id).await.unwrap().state,
        "failed"
    );
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert!(recordings.items[0].available);
}
