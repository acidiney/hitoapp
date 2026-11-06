#![cfg(unix)]

use hito_core::test_support::{media_tool, support};
use hito_core::{CoreConfig, HostDeviceId, Search};
use std::path::Path;

fn peak(log: &Path) -> i32 {
    let mut active = 0;
    let mut peak = 0;
    for line in std::fs::read_to_string(log).unwrap().lines() {
        active += if line == "+" { 1 } else { -1 };
        peak = peak.max(active);
    }
    peak
}

async fn scan_twelve(parallelism: usize) -> (i32, i64) {
    let (dir, _original, actor, library, music) = support::library("server").await;
    for index in 0..12 {
        support::write_wav(&music.join(format!("track-{index:02}.wav")), index as i16);
    }
    let log = dir.path().join("overlap");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "printf '+\\n' >> {log}\nsleep 0.3\nprintf -- '-\\n' >> {log}\nexec /usr/bin/ffprobe \"$@\"",
            log = media_tool::quote(&log),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = tool;
    config.media_tools.parallelism = parallelism;
    let hito = hito_core::open(config).await.unwrap();
    support::scan(&hito, &actor, &library).await;
    let total = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .total;
    (peak(&log), total)
}

#[tokio::test]
async fn a_scan_inspects_several_files_at_once_up_to_the_configured_limit() {
    let (peak, total) = scan_twelve(4).await;
    assert_eq!(total, 12);
    assert!(
        (3..=4).contains(&peak),
        "peak concurrent inspections: {peak}"
    );
}

#[tokio::test]
async fn a_parallelism_of_one_inspects_files_one_at_a_time() {
    let (peak, total) = scan_twelve(1).await;
    assert_eq!(total, 12);
    assert_eq!(peak, 1);
}
