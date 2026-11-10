use super::fixture::{Library, pid, until};
use hito_core::test_support::media_tool;
use hito_playback::{Command, Error, Session, Status};
use std::path::{Path, PathBuf};

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn a_failed_pcm_process_is_a_playback_failure_even_after_it_emits_valid_samples() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let f = Library::configured(|config, tools| {
        config.media_tools.ffmpeg = media_tool::script(tools, "ffmpeg", &format!("case \"$*\" in *pcm_f32le*) echo $$ > {}; head -c 38400 /dev/zero; exit 17;; esac\nexec /usr/bin/ffmpeg \"$@\"", media_tool::quote(&marker)));
    }).await;
    let session = Session::open(f.grant().await).await.unwrap();
    let child = pid(&marker).await;
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| state.status == Status::Failed).await;
    assert_eq!(
        session.snapshot().error,
        Some(Error::Native(hito_audio::Error::Decode))
    );
    assert!(!session.snapshot().ended);
    session.close().await.unwrap();
    assert!(!Path::new(&format!("/proc/{child}")).exists());
}

#[tokio::test]
#[ignore = "requires the isolated clocked audio monitor"]
async fn missing_decoder_tools_fail_safely_and_supported_sources_still_use_symphonia() {
    let witness = tempfile::tempdir().unwrap();
    let marker = witness.path().join("child");
    let mut tool_path = PathBuf::new();
    let f = Library::configured(|config, tools| {
        let tool = media_tool::script(tools, "ffmpeg", &format!("case \"$*\" in *pcm_f32le*) echo $$ > {}; exit 17;; esac\nexec /usr/bin/ffmpeg \"$@\"", media_tool::quote(&marker)));
        tool_path = tool.clone();
        config.media_tools.ffmpeg = tool;
    }).await;
    let wave = f.wave().await;
    std::fs::remove_file(tool_path).unwrap();
    assert!(matches!(
        Session::open(f.grant().await).await,
        Err(Error::Native(hito_audio::Error::Unsupported))
    ));
    assert!(!marker.exists());
    let session = Session::open(wave).await.unwrap();
    session.command(Command::Resume).await.unwrap();
    until(&session, |state| {
        state.position > std::time::Duration::from_millis(100)
    })
    .await;
    assert!(!marker.exists());
    session.close().await.unwrap();
    assert_eq!(std::fs::read(&f.path).unwrap(), f.original);
}
