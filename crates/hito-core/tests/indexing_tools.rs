#[cfg(unix)]
#[path = "fixtures/media_tool.rs"]
mod media_tool;
mod support;

use hito_core::{CoreConfig, HostDeviceId, Search};

#[tokio::test]
async fn a_missing_media_tool_fails_the_scan_without_hiding_verified_audio() {
    let (dir, original, actor, library, music) = support::library("server").await;
    support::write_wav(&music.join("track.wav"), 0);
    support::scan(&original, &actor, &library).await;
    let track = original
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = dir.path().join("missing-private-tool");
    let hito = hito_core::open(config).await.unwrap();
    let job = hito
        .storage
        .enqueue_scan(&actor, &library)
        .await
        .unwrap()
        .remove(0);
    assert!(hito.storage.run_next_job().await.unwrap());
    let job = hito.storage.job(&actor, &job.id).await.unwrap();
    assert_eq!(job.state, "failed");
    assert!(!job.error.unwrap().contains("missing-private-tool"));
    let current = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, track.id);
    assert!(current.items[0].available);
}

#[cfg(unix)]
#[tokio::test]
async fn unchanged_checksums_reuse_inspection_but_retagging_invalidates_the_cache() {
    use lofty::{
        prelude::{Accessor, TagExt},
        tag::{Tag, TagType},
    };
    let (dir, _original, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    support::write_wav(&path, 0);
    let calls = dir.path().join("probe-calls");
    let tool = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "printf 'inspect\\n' >> {}\nexec /usr/bin/ffprobe \"$@\"",
            media_tool::quote(&calls),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = tool;
    let hito = hito_core::open(config).await.unwrap();
    support::scan(&hito, &actor, &library).await;
    let original = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
    support::scan(&hito, &actor, &library).await;
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Changed tags".into());
    tag.save_to_path(&path, Default::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 2);
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.items[0].id, original);
    assert_eq!(recordings.items[0].title, "Changed tags");
}

#[cfg(unix)]
#[tokio::test]
async fn decoder_error_diagnostics_reject_a_digest_even_when_the_process_exits_successfully() {
    let (dir, original, actor, library, music) = support::library("server").await;
    let path = music.join("track.wav");
    support::write_wav(&path, 0);
    support::scan(&original, &actor, &library).await;
    let recording = original
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let tool = media_tool::script(
        dir.path(),
        "ffmpeg",
        "/usr/bin/ffmpeg \"$@\"\nresult=$?\nprintf 'PRIVATE_DECODER_ERROR\\n' >&2\nexit \"$result\"",
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffmpeg = tool;
    let hito = hito_core::open(config).await.unwrap();
    support::write_wav(&path, 1);
    support::scan(&hito, &actor, &library).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert_eq!(recordings.items[0].id, recording);
    assert!(!recordings.items[0].available);
}

#[cfg(unix)]
#[tokio::test]
async fn audio_inspection_reuses_completed_unsupported_profiles() {
    let (dir, _original, actor, library, music) = support::library("server").await;
    support::write_wav(&music.join("track.wav"), 0);
    let probe_calls = dir.path().join("probe-calls");
    let decoder_calls = dir.path().join("decoder-calls");
    let metadata = r#"{"streams":[{"codec_name":"pcm_s16le","sample_rate":"8000","channels":65}],"format":{"duration":"1"}}"#;
    let probe = media_tool::script(
        dir.path(),
        "ffprobe",
        &format!(
            "printf 'inspect\\n' >> {}\nprintf '%s\\n' '{}'",
            media_tool::quote(&probe_calls),
            metadata,
        ),
    );
    let decoder = media_tool::script(
        dir.path(),
        "ffmpeg",
        &format!(
            "printf 'decode\\n' >> {}\nexec /usr/bin/ffmpeg \"$@\"",
            media_tool::quote(&decoder_calls),
        ),
    );
    let mut config = CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        HostDeviceId::new("server").unwrap(),
    );
    config.media_tools.ffprobe = probe;
    config.media_tools.ffmpeg = decoder;
    let hito = hito_core::open(config).await.unwrap();
    support::scan(&hito, &actor, &library).await;
    support::scan(&hito, &actor, &library).await;
    assert_eq!(
        std::fs::read_to_string(&probe_calls)
            .unwrap()
            .lines()
            .count(),
        1,
    );
    assert!(!decoder_calls.exists());
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 1);
    assert!(recordings.items[0].available);
}
