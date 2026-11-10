#[path = "native_pcm/fixture.rs"]
#[allow(dead_code)]
mod fixture;

use hito_core::{Credentials, Error};
use std::{path::Path, time::Duration};

async fn pid(path: &Path) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(path) {
                break pid.trim().to_owned();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn closing_a_retained_opening_joins_held_probe_and_held_first_audio() {
    for stage in ["ffprobe", "ffmpeg"] {
        let witness = tempfile::tempdir().unwrap();
        let pid_file = witness.path().join("opening-pid");
        let library = fixture::library(2, |config, tools| {
            config.conversion_parallelism = 1;
            let marker = hito_core::test_support::media_tool::quote(&pid_file);
            let pattern = if stage == "ffprobe" { "stream=codec_name,sample_rate,channels,duration" } else { "pcm_f32le" };
            let tool = hito_core::test_support::media_tool::script(tools, stage, &format!("case \"$*\" in *{pattern}*) echo $$ > {marker}; exec sleep 30;; esac\nexec /usr/bin/{stage} \"$@\""));
            if stage == "ffprobe" { config.media_tools.ffprobe = tool; } else { config.media_tools.ffmpeg = tool; }
        }).await;
        let grant = library.grant().await;
        let mut opening = grant.begin_pcm(Duration::ZERO).unwrap();
        let first = pid(&pid_file).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(25), opening.ready())
                .await
                .is_err()
        );
        assert!(Path::new(&format!("/proc/{first}")).exists());
        tokio::time::timeout(Duration::from_secs(2), opening.close())
            .await
            .unwrap()
            .unwrap();
        assert!(
            !Path::new(&format!("/proc/{first}")).exists(),
            "{stage} must be reaped before close returns"
        );
        std::fs::remove_file(&pid_file).unwrap();
        let next = grant.begin_pcm(Duration::ZERO).unwrap();
        let second = pid(&pid_file).await;
        next.close().await.unwrap();
        assert!(!Path::new(&format!("/proc/{second}")).exists());
    }
}

#[tokio::test]
async fn ownership_transfer_disarms_the_opening_and_repeated_ready_is_safe() {
    let library = fixture::library(2, fixture::no_configuration).await;
    let mut opening = library.grant().await.begin_pcm(Duration::ZERO).unwrap();
    let mut stream = opening.ready().await.unwrap();
    assert!(opening.ready().await.is_err());
    opening.close().await.unwrap();
    let mut count = 0;
    while let Some(block) = stream.next_block().await.unwrap() {
        count += block.samples.len();
    }
    assert_eq!(count, 96_000);
    stream.close().await.unwrap();
}

#[tokio::test]
async fn revocation_during_probe_joins_the_process_before_returning_authority_failure() {
    let witness = tempfile::tempdir().unwrap();
    let pid_file = witness.path().join("opening-pid");
    let library = fixture::library(2, |config, tools| {
        let marker = hito_core::test_support::media_tool::quote(&pid_file);
        config.media_tools.ffprobe = hito_core::test_support::media_tool::script(tools, "ffprobe", &format!("case \"$*\" in *stream=codec_name,sample_rate,channels,duration*) echo $$ > {marker}; exec sleep 30;; esac\nexec /usr/bin/ffprobe \"$@\""));
    }).await;
    let session = library
        .services
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = library
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let recording = library
        .services
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let grant = library
        .services
        .playback
        .playback_source(&actor, &recording, &library.source)
        .await
        .unwrap();
    let mut opening = grant.begin_pcm(Duration::ZERO).unwrap();
    let child = pid(&pid_file).await;
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(2), opening.ready())
        .await
        .unwrap();
    assert!(matches!(ready, Err(Error::Unauthorized | Error::NotFound)));
    assert!(!Path::new(&format!("/proc/{child}")).exists());
    assert!(opening.ready().await.is_err());
    opening.close().await.unwrap();
}

#[tokio::test]
async fn replacing_the_path_during_probe_cannot_replace_the_retained_audio_file() {
    let witness = tempfile::tempdir().unwrap();
    let hold = witness.path().join("hold");
    let entered = witness.path().join("entered");
    let release = witness.path().join("release");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&release)
            .status()
            .unwrap()
            .success()
    );
    let library = fixture::library(2, |config, tools| {
        let quote = hito_core::test_support::media_tool::quote;
        config.media_tools.ffprobe = hito_core::test_support::media_tool::script(tools, "ffprobe", &format!("case \"$*\" in *stream=codec_name,sample_rate,channels,duration*) if [ -f {} ]; then echo $$ > {}; IFS= read -r release < {}; fi;; esac\nexec /usr/bin/ffprobe \"$@\"", quote(&hold), quote(&entered), quote(&release)));
    }).await;
    let grant = library.grant().await;
    let mut before = grant.open_pcm(Duration::ZERO).await.unwrap();
    let mut expected = Vec::new();
    while let Some(block) = before.next_block().await.unwrap() {
        expected.extend(block.samples);
    }
    before.close().await.unwrap();
    let replacement = library.directory.path().join("music/replacement.opus");
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000:duration=2",
                "-c:a",
                "libopus",
                "-ac",
                "2"
            ])
            .arg(&replacement)
            .status()
            .unwrap()
            .success()
    );
    let replacement_bytes = std::fs::read(&replacement).unwrap();
    std::fs::write(&hold, b"hold").unwrap();
    let mut opening = grant.begin_pcm(Duration::ZERO).unwrap();
    pid(&entered).await;
    let original = library
        .directory
        .path()
        .join("music/private-recording.opus");
    std::fs::rename(&replacement, &original).unwrap();
    std::fs::write(&release, b"resume\n").unwrap();
    let mut retained = opening.ready().await.unwrap();
    assert_eq!(retained.format().channels, 1);
    let mut actual = Vec::new();
    while let Some(block) = retained.next_block().await.unwrap() {
        actual.extend(block.samples);
    }
    retained.close().await.unwrap();
    assert!(
        actual == expected,
        "the retained handle must decode the originally inspected audio"
    );
    assert_eq!(std::fs::read(original).unwrap(), replacement_bytes);
}
