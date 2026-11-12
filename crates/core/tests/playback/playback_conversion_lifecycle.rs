use crate::conversion_support::*;
use hito_core::test_support::media_tool;
use hito_core::{Credentials, Error, QualityAudio, StreamQuality};
use std::{path::Path, time::Duration};

#[tokio::test]
async fn a_signed_out_session_cannot_convert() {
    let library = library(5, "flac", flac, no_configuration).await;
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
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    let result = library
        .services
        .playback
        .open_quality_audio(
            &actor,
            &library.source,
            request(StreamQuality::DataSaver, 0),
        )
        .await;
    assert!(matches!(result, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn signing_out_ends_a_running_conversion() {
    let library = library(30, "flac", flac, no_configuration).await;
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
    let QualityAudio::Converted(mut converted) = library
        .services
        .playback
        .open_quality_audio(
            &actor,
            &library.source,
            request(StreamQuality::DataSaver, 0),
        )
        .await
        .unwrap()
    else {
        panic!("the source must be converted");
    };
    let mut body = converted.body.take().unwrap();
    assert!(body.chunk().await.unwrap().is_some());
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    let mut failed = false;
    for _ in 0..200 {
        match body.chunk().await {
            Err(_) => {
                failed = true;
                break;
            }
            Ok(None) => break,
            Ok(Some(_)) => {}
        }
    }
    assert!(failed, "a revoked session must stop the conversion");
}

#[tokio::test]
async fn one_account_runs_at_most_three_conversions_at_once() {
    let library = library(30, "flac", flac, |config, _| {
        config.conversion_parallelism = 8
    })
    .await;
    let mut running = Vec::new();
    for _ in 0..3 {
        running.push(
            open(&library, request(StreamQuality::DataSaver, 0))
                .await
                .unwrap(),
        );
    }
    let fourth = open(&library, request(StreamQuality::DataSaver, 0)).await;
    assert!(matches!(fourth, Err(Error::ProviderBusy(_))));
    running.pop();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn the_host_limit_follows_conversion_parallelism() {
    let library = library(30, "flac", flac, |config, _| {
        config.conversion_parallelism = 1
    })
    .await;
    let first = open(&library, request(StreamQuality::DataSaver, 0))
        .await
        .unwrap();
    assert!(matches!(
        open(&library, request(StreamQuality::DataSaver, 0)).await,
        Err(Error::ProviderBusy(_))
    ));
    drop(first);
}

#[tokio::test]
async fn dropping_the_stream_kills_the_converter_process() {
    let witness = tempfile::tempdir().unwrap();
    let pid_file = witness.path().join("pid");
    let library = library(60, "flac", flac, |config, scratch| {
        config.media_tools.ffmpeg = media_tool::script(
            scratch,
            "ffmpeg",
            &format!(
                "case \"$*\" in *adts*) echo $$ > {};; esac\nexec /usr/bin/ffmpeg \"$@\"",
                media_tool::quote(&pid_file)
            ),
        );
    })
    .await;
    let QualityAudio::Converted(mut converted) =
        open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .unwrap()
    else {
        panic!("the source must be converted");
    };
    let mut body = converted.body.take().unwrap();
    assert!(body.chunk().await.unwrap().is_some());
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .to_owned();
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    drop(body);
    drop(converted);
    let mut gone = false;
    for _ in 0..100 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        if stat.is_empty() || stat.contains(") Z ") {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone, "the converter must not outlive its client");
    drop(library.directory);
}

#[tokio::test]
async fn logout_before_the_first_read_does_not_deliver_prefetched_audio() {
    let library = library(5, "flac", flac, no_configuration).await;
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
    let QualityAudio::Converted(mut converted) = library
        .services
        .playback
        .open_quality_audio(
            &actor,
            &library.source,
            request(StreamQuality::DataSaver, 0),
        )
        .await
        .unwrap()
    else {
        panic!("the source must be converted");
    };
    let mut body = converted.body.take().unwrap();
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        body.chunk().await,
        Err(Error::Unauthorized | Error::NotFound)
    ));
    assert!(body.chunk().await.unwrap().is_none());
}

#[tokio::test]
async fn a_converter_that_exits_unsuccessfully_never_reports_successful_eof() {
    let library = library(5, "flac", flac, |config, scratch| {
        config.media_tools.ffmpeg = media_tool::script(scratch, "ffmpeg", 
            "case \"$*\" in *adts*) /usr/bin/ffmpeg \"$@\"; exit 7;; esac\nexec /usr/bin/ffmpeg \"$@\"");
    }).await;
    let QualityAudio::Converted(mut converted) =
        open(&library, request(StreamQuality::DataSaver, 0))
            .await
            .unwrap()
    else {
        panic!("the source must be converted");
    };
    let mut body = converted.body.take().unwrap();
    assert!(body.chunk().await.unwrap().is_some());
    let terminal = loop {
        match body.chunk().await {
            Ok(Some(_)) => {}
            result => break result,
        }
    };
    assert!(
        terminal.is_err(),
        "failed conversion must not look like complete audio"
    );
    assert!(body.chunk().await.unwrap().is_none());
}

#[tokio::test]
async fn limiting_scan_workers_does_not_silently_limit_playback_conversions() {
    let library = library(30, "flac", flac, |config, _| {
        config.media_tools.parallelism = 1;
    })
    .await;
    let _first = open(&library, request(StreamQuality::DataSaver, 0))
        .await
        .unwrap();
    let second = open(&library, request(StreamQuality::DataSaver, 0)).await;
    assert!(matches!(second, Ok(QualityAudio::Converted(_))));
}

#[tokio::test]
async fn revocation_stops_a_converter_and_releases_capacity_without_consumer_polling() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("converter");
    let library = library(60, "flac", flac, |config, scratch| {
        config.conversion_parallelism = 1;
        config.media_tools.ffmpeg = media_tool::script(
            scratch,
            "ffmpeg",
            &format!(
                "case \"$*\" in *adts*) echo $$ > {};; esac\nexec /usr/bin/ffmpeg \"$@\"",
                media_tool::quote(&marker)
            ),
        );
    })
    .await;
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
    let audio = library
        .services
        .playback
        .open_quality_audio(
            &actor,
            &library.source,
            request(StreamQuality::DataSaver, 0),
        )
        .await
        .unwrap();
    assert!(matches!(&audio, QualityAudio::Converted(_)));
    let pid = std::fs::read_to_string(&marker).unwrap();
    let process = format!("/proc/{}", pid.trim());
    assert!(Path::new(&process).exists());
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while Path::new(&process).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the revoked converter must be joined even without another read");
    assert!(matches!(
        open(&library, request(StreamQuality::DataSaver, 0)).await,
        Ok(QualityAudio::Converted(_))
    ));
    drop(audio);
}

#[tokio::test]
async fn revocation_during_probe_never_starts_a_converter() {
    let witness = tempfile::tempdir().unwrap();
    let probing = witness.path().join("probing");
    let converted = witness.path().join("converted");
    let library = library(5, "flac", flac, |config, scratch| {
        config.media_tools.ffprobe = media_tool::script(scratch, "ffprobe", &format!(
            "case \"$*\" in *stream=codec_name,bit_rate:format=bit_rate*) echo $$ > {}; exec sleep 30;; esac\nexec /usr/bin/ffprobe \"$@\"", media_tool::quote(&probing)));
        config.media_tools.ffmpeg = media_tool::script(scratch, "ffmpeg", &format!(
            "case \"$*\" in *adts*) touch {};; esac\nexec /usr/bin/ffmpeg \"$@\"", media_tool::quote(&converted)));
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
    let playback = library.services.playback.clone();
    let source = library.source.clone();
    let pending = tokio::spawn(async move {
        playback
            .open_quality_audio(&actor, &source, request(StreamQuality::DataSaver, 0))
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !probing.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the conversion probe must begin");
    let pid = std::fs::read_to_string(&probing).unwrap();
    library
        .services
        .identity
        .logout(&session.token)
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(Error::Unauthorized | Error::NotFound)));
    assert!(!converted.exists());
    assert!(!Path::new(&format!("/proc/{}", pid.trim())).exists());
}
