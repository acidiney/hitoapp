use super::fixture::*;
use hito_core::{Credentials, Error, QualityRequest, StreamQuality};
use std::{path::Path, time::Duration};

async fn pid_file(path: &Path) -> String {
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

async fn gone(pid: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the decoder child must be reaped");
}

#[tokio::test]
async fn explicit_close_joins_a_stalled_decoder_and_releases_shared_conversion_capacity() {
    let witness = tempfile::tempdir().unwrap();
    let pid_path = witness.path().join("decoder-pid");
    let library = library(2, |config, tools| {
        config.conversion_parallelism = 1;
        let pid = hito_core::test_support::media_tool::quote(&pid_path);
        config.media_tools.ffmpeg = hito_core::test_support::media_tool::script(tools, "ffmpeg", &format!("case \"$*\" in *pcm_f32le*) echo $$ > {pid}; printf '\\000\\000\\200\\076'; exec sleep 30;; esac\nexec /usr/bin/ffmpeg \"$@\""));
    }).await;
    let grant = library.grant().await;
    let stream = grant.open_pcm(Duration::ZERO).await.unwrap();
    let pid = pid_file(&pid_path).await;
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    assert!(matches!(
        grant.open_pcm(Duration::ZERO).await,
        Err(Error::ProviderBusy(_))
    ));
    assert!(matches!(
        library
            .services
            .playback
            .open_quality_audio(
                &library.actor,
                &library.source,
                QualityRequest {
                    quality: StreamQuality::DataSaver,
                    start: Duration::ZERO,
                    original: Default::default(),
                }
            )
            .await,
        Err(Error::ProviderBusy(_))
    ));
    tokio::time::timeout(Duration::from_secs(1), stream.close())
        .await
        .unwrap()
        .unwrap();
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "close must join, not merely request cancellation"
    );
    let reopened = grant.open_pcm(Duration::ZERO).await.unwrap();
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn dropping_an_unread_stream_reaps_its_decoder_without_consumer_polling() {
    let witness = tempfile::tempdir().unwrap();
    let pid_path = witness.path().join("decoder-pid");
    let script = format!(
        "echo $$ > {}; printf '\\000\\000\\200\\076'; exec sleep 30",
        hito_core::test_support::media_tool::quote(&pid_path)
    );
    let library = scripted(&script).await;
    let stream = library
        .grant()
        .await
        .open_pcm(Duration::ZERO)
        .await
        .unwrap();
    let pid = pid_file(&pid_path).await;
    drop(stream);
    gone(&pid).await;
}

#[tokio::test]
async fn logout_reaps_an_idle_decoder_and_prevents_prefetched_audio_delivery() {
    let witness = tempfile::tempdir().unwrap();
    let pid_path = witness.path().join("decoder-pid");
    let script = format!(
        "echo $$ > {}; printf '\\000\\000\\200\\076'; exec sleep 30",
        hito_core::test_support::media_tool::quote(&pid_path)
    );
    let library = scripted(&script).await;
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
    let mut stream = grant.open_pcm(Duration::ZERO).await.unwrap();
    let pid = pid_file(&pid_path).await;
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    gone(&pid).await;
    let delivery = stream.next_block().await;
    assert!(
        matches!(delivery, Err(Error::Unauthorized | Error::NotFound)),
        "unexpected delivery state: {}",
        match delivery {
            Ok(Some(_)) => "audio",
            Ok(None) => "eof",
            Err(error) => error.code(),
        }
    );
    stream.close().await.unwrap();
    assert!(matches!(
        grant.open_pcm(Duration::ZERO).await,
        Err(Error::Unauthorized | Error::NotFound)
    ));
}

#[tokio::test]
async fn cancelled_short_reads_do_not_lose_prefetched_or_received_samples() {
    let library = scripted("printf '\\000\\000\\200\\076'; sleep 0.08; printf '\\000\\000\\000\\077'; sleep 0.08; printf '\\000\\000\\100\\077'; exit 0").await;
    let mut stream = library
        .grant()
        .await
        .open_pcm(Duration::ZERO)
        .await
        .unwrap();
    let mut values = Vec::new();
    let mut interrupted = 0;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match tokio::time::timeout(Duration::from_millis(5), stream.next_block()).await {
                Err(_) => interrupted += 1,
                Ok(Ok(Some(block))) => values.extend(block.samples),
                Ok(Ok(None)) => break,
                Ok(Err(error)) => panic!("unexpected stream error: {error:?}"),
            }
        }
    })
    .await
    .unwrap();
    assert!(interrupted > 0);
    assert_eq!(values, [0.25, 0.5, 0.75]);
    stream.close().await.unwrap();
}
