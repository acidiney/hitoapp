use hito_core::{
    Actor, CoreConfig, Credentials, PlaybackSource, Search, Services, test_support::support,
};
use hito_playback::{Session, Snapshot};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub struct Library {
    _directory: tempfile::TempDir,
    _tools: tempfile::TempDir,
    library: String,
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    pub recording: String,
    pub source: String,
    pub path: PathBuf,
    pub original: Vec<u8>,
}

impl Library {
    pub async fn start() -> Self {
        Self::configured(|_, _| {}).await
    }

    pub async fn configured(configure: impl FnOnce(&mut CoreConfig, &Path)) -> Self {
        let tools = tempfile::tempdir().unwrap();
        let (directory, services, admin, library, music) =
            support::library_configured("native-fallback", |config| {
                configure(config, tools.path())
            })
            .await;
        let path = music.join("owned.opus");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=4",
                    "-c:a",
                    "libopus",
                    "-ac",
                    "1"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let original = std::fs::read(&path).unwrap();
        support::scan(&services, &admin, &library).await;
        let session = services
            .identity
            .login(Credentials {
                username: "admin".into(),
                password: "indexing test password".into(),
            })
            .await
            .unwrap();
        let actor = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
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
        Self {
            _directory: directory,
            _tools: tools,
            library,
            services,
            actor,
            token: session.token,
            recording,
            source,
            path,
            original,
        }
    }

    pub async fn wave(&self) -> PlaybackSource {
        self.wave_for(6.).await
    }

    pub async fn wave_for(&self, seconds: f64) -> PlaybackSource {
        let path = self.path.parent().unwrap().join("current.wav");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("sine=frequency=220:sample_rate=48000:duration={seconds}"),
                    "-c:a",
                    "pcm_s16le",
                    "-ac",
                    "1"
                ])
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
        support::scan(&self.services, &self.actor, &self.library).await;
        let recording = self
            .services
            .catalog
            .recordings(&self.actor, Search::default())
            .await
            .unwrap()
            .items
            .into_iter()
            .find(|track| track.id != self.recording)
            .unwrap()
            .id;
        let source = self
            .services
            .catalog
            .sources(&self.actor, &recording)
            .await
            .unwrap()
            .remove(0)
            .id;
        self.services
            .playback
            .playback_source(&self.actor, &recording, &source)
            .await
            .unwrap()
    }

    pub async fn rescan(&self) {
        support::scan(&self.services, &self.actor, &self.library).await;
    }

    pub async fn grant(&self) -> PlaybackSource {
        self.services
            .playback
            .playback_source(&self.actor, &self.recording, &self.source)
            .await
            .unwrap()
    }
}

pub async fn until(session: &Session, condition: impl Fn(&Snapshot) -> bool) {
    tokio::time::timeout(Duration::from_secs(4), async {
        while !condition(&session.snapshot()) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native playback state did not arrive");
}

pub async fn pid(path: &Path) -> String {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(path) {
                return pid.trim().to_owned();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("decoder process did not reach its owned gate")
}

pub async fn open_with_diagnostics(grant: PlaybackSource) -> Session {
    use tracing::instrument::WithSubscriber;
    let logs = hito_core::test_support::metadata_tag_logs::CapturedLogs::default();
    let captured = logs.clone();
    hito_testkit::logs::anchor_callsite_interest();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .with_writer(move || captured.clone())
            .finish(),
    );
    Session::open(grant)
        .with_subscriber(dispatch)
        .await
        .unwrap_or_else(|error| panic!("native open failed: {error:?}\n{}", logs.text()))
}
