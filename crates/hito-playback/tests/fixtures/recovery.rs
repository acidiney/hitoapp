#![allow(dead_code)]
#[path = "../../../hito-audio/tests/support/mod.rs"]
#[allow(dead_code)]
mod audio;
use crate::support;
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
        let (directory, services, actor, library, music) = support::library("recovery").await;
        let shared = services
            .storage
            .shared_directories(&actor)
            .await
            .unwrap()
            .remove(0)
            .id;
        let mut wave = audio::wave(48_000, 1, 48_000 * 10, |frame, _| {
            ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 13_000.) as i16
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
