use hito_core::{CoreConfig, Credentials, HostDeviceId};
use std::{path::PathBuf, time::Instant};

#[tokio::test]
#[ignore = "opt-in measurement over a generated library; set BENCH_LIBRARY"]
async fn scan_speed_over_a_generated_library() {
    let library_root = PathBuf::from(std::env::var("BENCH_LIBRARY").expect("BENCH_LIBRARY"));
    let out =
        PathBuf::from(std::env::var("BENCH_OUT").unwrap_or_else(|_| "/tmp/scan-speed.json".into()));
    let dir = tempfile::tempdir_in("/dev/shm").unwrap();
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        library_root,
        HostDeviceId::new("bench-scan").unwrap(),
    );
    if let Ok(value) = std::env::var("BENCH_PARALLELISM") {
        config.media_tools.parallelism = value.parse().unwrap();
    }
    let hito = hito_core::open(config).await.unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "bench-admin".into(),
            password: "scan benchmark password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let library = hito.catalog.libraries(&actor).await.unwrap().remove(0).id;
    let mut runs = Vec::new();
    for name in ["first_scan", "rescan"] {
        let started = Instant::now();
        let job = hito
            .storage
            .enqueue_scan(&actor, &library)
            .await
            .unwrap()
            .remove(0);
        assert!(hito.storage.run_next_job().await.unwrap());
        let job = hito.storage.job(&actor, &job.id).await.unwrap();
        assert_eq!(job.state, "complete", "{:?}", job.error);
        let seconds = started.elapsed().as_secs_f64();
        let total = hito
            .catalog
            .recordings(
                &actor,
                hito_core::Search {
                    limit: Some(1),
                    ..hito_core::Search::default()
                },
            )
            .await
            .unwrap()
            .total;
        runs.push(serde_json::json!({"run": name, "seconds": seconds, "recordings": total}));
        eprintln!("SCAN_SPEED {name} seconds={seconds:.2} recordings={total}");
    }
    std::fs::write(out, serde_json::to_vec_pretty(&runs).unwrap()).unwrap();
}
