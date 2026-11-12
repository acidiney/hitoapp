use hito_core::{Actor, CoreConfig, PlaybackSource, Search, Services, test_support::support};
use std::{path::Path, process::Command};

pub struct Library {
    pub directory: tempfile::TempDir,
    _tools: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub source: String,
    recording: String,
}

pub fn opus(path: &Path, seconds: u32) {
    assert!(
        Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!(
                "sine=frequency=440:sample_rate=48000:duration={seconds}"
            ))
            .args(["-c:a", "libopus", "-ac", "1"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

pub fn no_configuration(_: &mut CoreConfig, _: &Path) {}

pub async fn library(seconds: u32, configure: impl FnOnce(&mut CoreConfig, &Path)) -> Library {
    let tools = tempfile::tempdir().unwrap();
    let (directory, services, actor, library, music) =
        support::library_configured("native-pcm", |config| configure(config, tools.path())).await;
    opus(&music.join("private-recording.opus"), seconds);
    support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let source = services
        .catalog
        .sources(&actor, &recording)
        .await
        .unwrap()
        .remove(0)
        .id;
    Library {
        directory,
        _tools: tools,
        services,
        actor,
        source,
        recording,
    }
}

pub async fn scripted(body: &str) -> Library {
    library(2, |config, tools| {
        config.media_tools.ffmpeg = hito_core::test_support::media_tool::script(
            tools,
            "ffmpeg",
            &format!("case \"$*\" in *pcm_f32le*) {body};; esac\nexec /usr/bin/ffmpeg \"$@\""),
        );
    })
    .await
}

impl Library {
    pub async fn grant(&self) -> PlaybackSource {
        self.services
            .playback
            .playback_source(&self.actor, &self.recording, &self.source)
            .await
            .unwrap()
    }
}
