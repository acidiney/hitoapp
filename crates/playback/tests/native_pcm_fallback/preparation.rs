use super::fixture::{Library, pid, until};
use hito_core::test_support::media_tool;
use hito_playback::{Command, Session};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn held_next_pcm_startup_does_not_block_current_seek_or_joined_cancellation() {
    for stage in ["ffprobe", "ffmpeg"] {
        let witness = tempfile::tempdir().unwrap();
        let marker = witness.path().join("child");
        let f = Library::configured(|config, tools| {
            config.conversion_parallelism = 1;
            let pattern = if stage == "ffprobe" { "stream=codec_name,sample_rate,channels,duration" } else { "pcm_f32le" };
            let tool = media_tool::script(tools, stage, &format!("case \"$*\" in *{pattern}*) echo $$ > {}; exec sleep 30;; esac\nexec /usr/bin/{stage} \"$@\"", media_tool::quote(&marker)));
            if stage == "ffprobe" { config.media_tools.ffprobe = tool; } else { config.media_tools.ffmpeg = tool; }
        }).await;
        let session = Arc::new(Session::open(f.wave().await).await.unwrap());
        session.command(Command::Resume).await.unwrap();
        until(&session, |state| {
            state.position > Duration::from_millis(100)
        })
        .await;
        let next = f.grant().await;
        let retained = session.clone();
        let pending = tokio::spawn(async move { retained.prepare_next(next).await });
        let child = pid(&marker).await;
        let started = Instant::now();
        tokio::time::timeout(
            Duration::from_millis(500),
            session.command(Command::Seek(Duration::from_millis(750))),
        )
        .await
        .expect("current seek must not wait for the next decoder")
        .unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(session.snapshot().playing);
        assert_eq!(session.snapshot().track, 0);
        let started = Instant::now();
        assert!(
            tokio::time::timeout(Duration::from_millis(500), session.cancel_next())
                .await
                .expect("cancel must join the held next decoder promptly")
                .unwrap()
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(pending.await.unwrap().is_err());
        assert!(
            !Path::new(&format!("/proc/{child}")).exists(),
            "cancel returned before the next decoder was reaped"
        );
        until(&session, |state| {
            state.position > Duration::from_millis(850)
        })
        .await;
        let session = Arc::try_unwrap(session).ok().unwrap();
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn dropping_an_unobserved_prepare_receipt_preserves_the_adopted_sources_grant() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let release = witness.path().join("release");
    let f = Library::configured(|config, tools| {
        let code = format!("import os,sys,time,pathlib\npathlib.Path({:?}).write_text(str(os.getpid()))\nwhile not pathlib.Path({:?}).exists(): time.sleep(0.01)\nos.execv('/usr/bin/ffprobe',['/usr/bin/ffprobe',*sys.argv[1:]])", marker.to_string_lossy(), release.to_string_lossy());
        let code = format!("'{}'", code.replace('\'', "'\\''"));
        config.media_tools.ffprobe = media_tool::script(tools, "ffprobe", &format!("case \"$*\" in *stream=codec_name,sample_rate,channels,duration*) exec /usr/bin/python3 -c {code} \"$@\";; esac\nexec /usr/bin/ffprobe \"$@\""));
    }).await;
    let session = Session::open(f.wave_for(0.5).await).await.unwrap();
    let next = f.grant().await;
    let mut preparing = Box::pin(session.prepare_next(next));
    tokio::select! {
        biased;
        result = &mut preparing => panic!("preparation completed before its owned gate: {result:?}"),
        _ = pid(&marker) => (),
    }
    std::fs::write(&release, b"ready").unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.track == 1 && state.position > Duration::from_millis(100)
    })
    .await;
    drop(preparing);
    std::fs::remove_file(f.path.parent().unwrap().join("current.wav")).unwrap();
    f.rescan().await;
    let before = session.snapshot().position;
    until(&session, |state| {
        state.position > before + Duration::from_millis(150)
            || state.status != hito_playback::Status::Ready
    })
    .await;
    assert_eq!(
        session.snapshot().status,
        hito_playback::Status::Ready,
        "retiring the old input revoked the adopted source"
    );
    assert!(session.snapshot().playing);
    f.services.identity.logout(&f.token).await.unwrap();
    until(&session, |state| {
        state.status == hito_playback::Status::Revoked
    })
    .await;
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn dropping_an_uncommitted_prepare_observer_joins_only_the_next_input() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let f = Library::configured(|config, tools| {
        config.media_tools.ffmpeg = media_tool::script(tools, "ffmpeg", &format!("case \"$*\" in *pcm_f32le*) echo $$ > {}; exec sleep 30;; esac\nexec /usr/bin/ffmpeg \"$@\"", media_tool::quote(&marker)));
    }).await;
    let session = Session::open(f.wave().await).await.unwrap();
    session.command(Command::Resume).await.unwrap();
    let next = f.grant().await;
    let mut preparing = Box::pin(session.prepare_next(next));
    let child = tokio::select! {
        result = &mut preparing => panic!("preparation completed before its owned gate: {result:?}"),
        child = pid(&marker) => child,
    };
    drop(preparing);
    tokio::time::timeout(Duration::from_millis(500), async {
        while Path::new(&format!("/proc/{child}")).exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("abandoned preparation did not join its child");
    assert!(session.snapshot().playing);
    assert_eq!(session.snapshot().track, 0);
    let before = session.snapshot().position;
    until(&session, |state| {
        state.position > before + Duration::from_millis(100)
    })
    .await;
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn cancelling_a_ready_pcm_successor_joins_it_before_acknowledging_removal() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let f = Library::configured(|config, tools| {
        config.conversion_parallelism = 1;
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
    let session = Session::open(f.wave_for(0.5).await).await.unwrap();
    session.prepare_next(f.grant().await).await.unwrap();
    let child = pid(&marker).await;
    assert!(Path::new(&format!("/proc/{child}")).exists());
    assert!(
        tokio::time::timeout(Duration::from_millis(500), session.cancel_next())
            .await
            .expect("ready successor cancellation did not join promptly")
            .unwrap()
    );
    assert!(!Path::new(&format!("/proc/{child}")).exists());
    assert!(!session.cancel_next().await.unwrap());
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| state.ended).await;
    assert_eq!(session.snapshot().track, 0);
    session.close().await.unwrap();
    let retry = f.grant().await.open_pcm(Duration::ZERO).await.unwrap();
    retry.close().await.unwrap();
}
