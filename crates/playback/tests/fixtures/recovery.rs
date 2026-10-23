#![allow(dead_code)]
use crate::support;
use hito_audio::test_support as audio;
use hito_core::{Actor, ConnectFolder, DirectoryRef, MusicFolder, Search, Services};

pub struct Recovery {
    pub directory: tempfile::TempDir,
    pub services: Services,
    pub actor: Actor,
    pub recording: String,
    pub first: MusicFolder,
}
impl Recovery {
    pub async fn start() -> Self {
        Self::start_with_wave(48_000, 48_000 * 10).await
    }
    pub async fn start_with_wave(rate: u32, frames: u32) -> Self {
        Self::start_signal(rate, frames, false).await
    }
    pub async fn start_split() -> Self {
        Self::start_signal(48_000, 48_000 * 10, true).await
    }
    async fn start_signal(rate: u32, frames: u32, split: bool) -> Self {
        let (directory, services, actor, library, music) = support::library("recovery").await;
        let shared = services
            .storage
            .shared_directories(&actor)
            .await
            .unwrap()
            .remove(0)
            .id;
        let mut wave = audio::wave(rate, 1, frames, |frame, _| {
            let frequency = if split && frame >= rate * 5 {
                880.
            } else {
                440.
            };
            ((f64::from(frame) * frequency * std::f64::consts::TAU / f64::from(rate)).sin()
                * 13_000.) as i16
        });
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut wave, &mut bytes).unwrap();
        let mut folders = Vec::new();
        let mut first_source = String::new();
        let mut recording = String::new();
        for component in ["primary", "backup"] {
            std::fs::create_dir(music.join(component)).unwrap();
            std::fs::write(music.join(component).join("track.wav"), &bytes).unwrap();
            let folder = services
                .storage
                .connect_folder(
                    &actor,
                    &library,
                    ConnectFolder {
                        directory: DirectoryRef {
                            directory_id: shared.clone(),
                            components: vec![component.into()],
                        },
                        name: None,
                    },
                )
                .await
                .unwrap();
            assert!(services.storage.run_next_job().await.unwrap());
            if recording.is_empty() {
                recording = services
                    .catalog
                    .recordings(&actor, Search::default())
                    .await
                    .unwrap()
                    .items
                    .remove(0)
                    .id;
                first_source = services
                    .catalog
                    .sources(&actor, &recording)
                    .await
                    .unwrap()
                    .remove(0)
                    .id;
            }
            folders.push(folder);
        }
        let sources = services.catalog.sources(&actor, &recording).await.unwrap();
        assert_eq!(sources.len(), 2);
        let first = folders.remove(if sources[0].id == first_source { 0 } else { 1 });
        Self {
            directory,
            services,
            actor,
            recording,
            first,
        }
    }
}
