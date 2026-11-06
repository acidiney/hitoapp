#![cfg(unix)]

use hito_core::test_support::{media_tool, support};
use hito_core::{CoreConfig, HostDeviceId, Search};
use std::{path::Path, time::Duration};

const LARGE_SAMPLES: u32 = 100_000;
const LARGE_BYTES: u64 = 44 + 2 * LARGE_SAMPLES as u64;

fn write_wav(path: &Path, samples: u32, offset: i16) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for sample in 0..samples {
        bytes.extend_from_slice(&((sample % 100) as i16 * 100 + offset).to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

struct Observed {
    peak_bytes: u64,
    peak_files: i32,
}

fn observed(log: &Path) -> Observed {
    let mut files = 0;
    let mut peak_files = 0;
    let mut peak_bytes = 0;
    for line in std::fs::read_to_string(log).unwrap().lines() {
        match line.split_once(' ') {
            Some(("+", bytes)) => {
                files += 1;
                peak_files = peak_files.max(files);
                peak_bytes = peak_bytes.max(bytes.parse().unwrap());
            }
            _ => files -= 1,
        }
    }
    Observed {
        peak_bytes,
        peak_files,
    }
}

fn temp_entries(temp: &Path) -> usize {
    std::fs::read_dir(temp).unwrap().count()
}

struct Scan {
    peak_bytes: u64,
    peak_files: i32,
    total: u64,
    leftovers: usize,
}

async fn scan(files: &[(u32, i16)], parallelism: usize, budget: Option<u64>, probe: &str) -> Scan {
    let (dir, _original, actor, library, music) = support::library("server").await;
    for (index, (samples, offset)) in files.iter().enumerate() {
        write_wav(
            &music.join(format!("track-{index:02}.wav")),
            *samples,
            *offset,
        );
    }
    let temp = dir.path().join("temp");
    std::fs::create_dir(&temp).unwrap();
    let log = dir.path().join("overlap");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "total=$(cat {temp}/hito-media-*/media 2>/dev/null | wc -c)\nprintf '+ %s\\n' \"$total\" >> {log}\nsleep 0.3\nprintf -- '-\\n' >> {log}\n{probe}\nexec /usr/bin/ffprobe \"$@\"",
            temp = media_tool::quote(&temp),
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
    config.media_tools.temp_dir = Some(temp.clone());
    if let Some(budget) = budget {
        config.media_tools.temp_budget = budget;
    }
    let hito = hito_core::open(config).await.unwrap();
    let job = hito
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(hito.storage.run_next_job().await.unwrap());
    let state = hito.storage.job(&actor, &job.id).await.unwrap().state;
    let total = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .total as u64;
    let seen = observed(&log);
    assert!(state == "complete" || probe.contains("exit 1"), "{state}");
    Scan {
        peak_bytes: seen.peak_bytes,
        peak_files: seen.peak_files,
        total,
        leftovers: temp_entries(&temp),
    }
}

#[tokio::test]
async fn large_files_never_hold_more_private_copy_bytes_than_the_budget() {
    let files: Vec<_> = (0..6).map(|index| (LARGE_SAMPLES, index)).collect();
    let budget = LARGE_BYTES * 5 / 2;
    let result = scan(&files, 6, Some(budget), "").await;
    assert_eq!(result.total, 6);
    assert!(
        result.peak_bytes <= budget,
        "peak copy bytes {} over budget {budget}",
        result.peak_bytes
    );
    assert!(
        result.peak_bytes >= LARGE_BYTES * 2,
        "peak copy bytes {} show no overlap",
        result.peak_bytes
    );
    assert_eq!(result.leftovers, 0);
}

#[tokio::test]
async fn a_file_larger_than_the_budget_is_inspected_alone() {
    let files: Vec<_> = (0..3).map(|index| (LARGE_SAMPLES, index)).collect();
    let result = scan(&files, 3, Some(LARGE_BYTES / 2), "").await;
    assert_eq!(result.total, 3);
    assert_eq!(result.peak_files, 1);
    assert_eq!(result.peak_bytes, LARGE_BYTES);
    assert_eq!(result.leftovers, 0);
}

#[tokio::test]
async fn small_files_still_run_in_parallel_under_the_default_budget() {
    let files: Vec<_> = (0..12).map(|index| (4000, index)).collect();
    let result = scan(&files, 4, None, "").await;
    assert_eq!(result.total, 12);
    assert!(
        (3..=4).contains(&result.peak_files),
        "files inspected together: {}",
        result.peak_files
    );
    assert_eq!(result.leftovers, 0);
}

#[tokio::test]
async fn failed_inspections_leave_no_private_copies() {
    let files: Vec<_> = (0..4).map(|index| (4000, index)).collect();
    let result = scan(&files, 4, None, "exit 1").await;
    assert_eq!(result.total, 0);
    assert_eq!(result.leftovers, 0);
}

#[tokio::test]
async fn stopping_a_scan_returns_its_budget_and_removes_its_copies() {
    let (dir, _original, actor, library, music) = support::library("server").await;
    for index in 0..3 {
        write_wav(
            &music.join(format!("track-{index}.wav")),
            LARGE_SAMPLES,
            index,
        );
    }
    let temp = dir.path().join("temp");
    std::fs::create_dir(&temp).unwrap();
    let marker = dir.path().join("started");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "if [ ! -e {marker} ]; then : > {marker}; exec sleep 300; fi\nexec /usr/bin/ffprobe \"$@\"",
            marker = media_tool::quote(&marker),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = tool;
    config.media_tools.parallelism = 3;
    config.media_tools.temp_dir = Some(temp.clone());
    config.media_tools.temp_budget = LARGE_BYTES;
    let hito = hito_core::open(config).await.unwrap();
    let job = hito
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    tokio::time::timeout(
        Duration::from_secs(10),
        hito.storage.run_next_job_until(async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        hito.storage.job(&actor, &job.id).await.unwrap().state,
        "queued"
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while temp_entries(&temp) > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Private copies must be removed when the scan stops");
    assert!(
        tokio::time::timeout(Duration::from_secs(30), hito.storage.run_next_job())
            .await
            .expect("A stopped scan must return its budget")
            .unwrap()
    );
    assert_eq!(
        hito.storage.job(&actor, &job.id).await.unwrap().state,
        "complete"
    );
    assert_eq!(temp_entries(&temp), 0);
}
