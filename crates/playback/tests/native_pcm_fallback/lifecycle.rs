use super::fixture::{Library, pid, until};
use hito_core::test_support::media_tool;
use hito_playback::{Command, Session, Status};
use std::{path::Path, time::Duration};

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn stopped_and_unobserved_startup_join_the_owned_probe_and_first_audio_process() {
    for (stage, abandon_close) in [
        ("ffprobe", false),
        ("ffmpeg", false),
        ("ffprobe", true),
        ("ffmpeg", true),
    ] {
        let witness = tempfile::tempdir().unwrap();
        let marker = witness.path().join("child");
        let f = Library::configured(|config, tools| {
            config.conversion_parallelism = 1;
            let pattern = if stage == "ffprobe" { "stream=codec_name,sample_rate,channels,duration" } else { "pcm_f32le" };
            let tool = media_tool::script(tools, stage, &format!("case \"$*\" in *{pattern}*) echo $$ > {}; exec sleep 30;; esac\nexec /usr/bin/{stage} \"$@\"", media_tool::quote(&marker)));
            if stage == "ffprobe" { config.media_tools.ffprobe = tool; } else { config.media_tools.ffmpeg = tool; }
        }).await;
        let grant = f.grant().await;
        let session = Session::start(grant.clone());
        let mut observed = session.subscribe();
        let child = pid(&marker).await;
        if abandon_close {
            let closing = tokio::spawn(session.close());
            tokio::task::yield_now().await;
            closing.abort();
            let _ = closing.await;
            tokio::time::timeout(
                Duration::from_secs(2),
                observed.wait_for(|state| state.status == Status::Stopped),
            )
            .await
            .unwrap()
            .unwrap();
        } else {
            session.stop();
            tokio::time::timeout(Duration::from_secs(2), session.close())
                .await
                .unwrap()
                .unwrap();
        }
        assert!(
            !Path::new(&format!("/proc/{child}")).exists(),
            "native startup returned before its {stage} process was reaped"
        );
        std::fs::remove_file(&marker).unwrap();
        let retry = grant.begin_pcm(Duration::ZERO).unwrap();
        let child = pid(&marker).await;
        retry.close().await.unwrap();
        assert!(!Path::new(&format!("/proc/{child}")).exists());
    }
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn logout_while_paused_joins_the_backpressured_decoder_without_another_command() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let f = Library::configured(|config, tools| {
        config.media_tools.ffmpeg = media_tool::script(
            tools,
            "ffmpeg",
            &format!(
                "case \"$*\" in *pcm_f32le*) echo $$ > {};; esac\nexec /usr/bin/ffmpeg \"$@\"",
                media_tool::quote(&marker)
            ),
        );
    })
    .await;
    let session = Session::open(f.grant().await).await.unwrap();
    let child = pid(&marker).await;
    assert!(Path::new(&format!("/proc/{child}")).exists());
    assert!(!session.snapshot().playing);
    f.services.identity.logout(&f.token).await.unwrap();
    until(&session, |state| state.status == Status::Revoked).await;
    assert!(!Path::new(&format!("/proc/{child}")).exists());
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_stalled_pcm_read_can_seek_and_stop_without_waiting_for_the_read_timeout() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let f = Library::configured(|config, tools| {
        config.conversion_parallelism = 1;
        config.media_tools.ffmpeg = media_tool::script(tools, "ffmpeg", &format!("case \"$*\" in *pcm_f32le*) echo $$ > {}; head -c 192000 /dev/zero; exec sleep 30;; esac\nexec /usr/bin/ffmpeg \"$@\"", media_tool::quote(&marker)));
    }).await;
    let session = Session::open(f.grant().await).await.unwrap();
    let child = pid(&marker).await;
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.position >= Duration::from_millis(910)
    })
    .await;
    tokio::time::timeout(
        Duration::from_millis(500),
        session.command(Command::Seek(Duration::from_millis(1_500))),
    )
    .await
    .expect("seek waited for stalled decoder output")
    .unwrap();
    assert!(!Path::new(&format!("/proc/{child}")).exists());
    let replacement = pid(&marker).await;
    assert_ne!(child, replacement);
    session.stop();
    tokio::time::timeout(Duration::from_millis(500), session.close())
        .await
        .expect("stop waited for stalled decoder output")
        .unwrap();
    assert!(!Path::new(&format!("/proc/{replacement}")).exists());
}
