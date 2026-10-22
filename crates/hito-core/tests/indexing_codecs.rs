use hito_core::test_support::support;

use hito_core::Search;
use std::{path::Path, process::Command};

fn encode(input: &Path, output: &Path, options: &[&str]) {
    assert!(
        Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(input)
            .args(options)
            .arg(output)
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn supported_containers_keep_identity_when_tags_are_rewritten_without_reencoding() {
    for (extension, codec) in [
        ("wav", "pcm_s16le"),
        ("flac", "flac"),
        ("mp3", "libmp3lame"),
        ("ogg", "libvorbis"),
        ("opus", "libopus"),
        ("m4a", "aac"),
        ("aac", "aac"),
        ("aiff", "pcm_s16be"),
        ("mp4", "aac"),
        ("wma", "wmav2"),
    ] {
        let (dir, hito, actor, library, music) = support::library("server").await;
        let input = dir.path().join("input.wav");
        support::write_wav(&input, 0);
        let path = music.join(format!("track.{extension}"));
        encode(&input, &path, &["-c:a", codec]);
        support::scan(&hito, &actor, &library).await;
        let before = hito
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(before.total, 1, "{extension}");
        let original = &before.items[0];
        let source = hito
            .catalog
            .sources(&actor, &original.id)
            .await
            .unwrap()
            .remove(0)
            .id;
        let bytes = std::fs::read(&path).unwrap();
        let tagged = dir.path().join(format!("tagged.{extension}"));
        let mut options = vec![
            "-map",
            "0:a:0",
            "-c:a",
            "copy",
            "-metadata",
            "title=Retagged audio",
        ];
        if extension == "aac" {
            options.extend(["-write_id3v2", "1"]);
        }
        encode(&path, &tagged, &options);
        assert_ne!(bytes, std::fs::read(&tagged).unwrap(), "{extension}");
        std::fs::rename(tagged, &path).unwrap();
        support::scan(&hito, &actor, &library).await;
        let after = hito
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap();
        assert_eq!(after.total, 1, "{extension}");
        assert_eq!(after.items[0].id, original.id, "{extension}");
        assert!(after.items[0].available, "{extension}");
        assert_eq!(
            hito.catalog.sources(&actor, &original.id).await.unwrap()[0].id,
            source,
            "{extension}"
        );
    }
}

fn pcm32(path: &Path, offset: i32) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&32036u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    for field in [1u16, 1] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    for field in [8000u32, 32000] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    for field in [4u16, 32] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&32000u32.to_le_bytes());
    for sample in 0..8000 {
        bytes.extend_from_slice(&(65536i32 + sample * 65536 + offset).to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn a_one_bit_difference_in_32_bit_pcm_does_not_merge_recordings() {
    let (_dir, hito, actor, library, music) = support::library("server").await;
    pcm32(&music.join("a.wav"), 0);
    pcm32(&music.join("b.wav"), 1);
    support::scan(&hito, &actor, &library).await;
    let recordings = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap();
    assert_eq!(recordings.total, 2);
    assert!(recordings.items.iter().all(|recording| recording.available));
}
