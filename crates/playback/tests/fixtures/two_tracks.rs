#![allow(dead_code)]
use hito_audio::test_support::{self as audio, continuity::counter};
use hito_core::{Actor, Credentials, PlaybackSource, Search, Services, test_support::support};

pub struct Track {
    pub recording: String,
    pub source: String,
}

pub struct TwoTracks {
    pub directory: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    pub music: std::path::PathBuf,
    pub library: String,
    pub first: Track,
    pub second: Track,
}

impl TwoTracks {
    pub async fn start(first_frames: u32, second_frames: u32) -> Self {
        assert_ne!(first_frames, second_frames);
        let (directory, services, admin, library, music) = support::library("gapless").await;
        for (name, start, frames) in [
            ("first.wav", 0, first_frames),
            ("second.wav", u64::from(first_frames), second_frames),
        ] {
            let mut wave = audio::wave(48_000, 2, frames, |frame, _| {
                counter(start + u64::from(frame))
            });
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut wave, &mut bytes).unwrap();
            std::fs::write(music.join(name), bytes).unwrap();
        }
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
        let recordings = services
            .catalog
            .recordings(&actor, Search::default())
            .await
            .unwrap()
            .items;
        assert_eq!(recordings.len(), 2);
        let mut tracks = Vec::new();
        for frames in [first_frames, second_frames] {
            let recording = recordings
                .iter()
                .find(|recording| recording.duration_ms == i64::from(frames) * 1000 / 48_000)
                .expect("recording with the fixture duration");
            let source = services
                .catalog
                .sources(&actor, &recording.id)
                .await
                .unwrap()
                .remove(0)
                .id;
            tracks.push(Track {
                recording: recording.id.clone(),
                source,
            });
        }
        let second = tracks.pop().unwrap();
        let first = tracks.pop().unwrap();
        Self {
            directory,
            services,
            actor,
            token: session.token,
            music,
            library,
            first,
            second,
        }
    }

    pub async fn grant(&self, track: &Track) -> PlaybackSource {
        self.services
            .playback
            .playback_source(&self.actor, &track.recording, &track.source)
            .await
            .unwrap()
    }
}
