#![allow(dead_code)]
use crate::support;
use hito_core::{Actor, Credentials, Search, Services};

pub struct LocalPlayback {
    pub directory: tempfile::TempDir,
    pub music: std::path::PathBuf,
    pub services: Services,
    pub actor: Actor,
    pub token: String,
    pub library: String,
    pub recording: String,
    pub source: String,
    pub original: Vec<u8>,
}

impl LocalPlayback {
    pub async fn start() -> Self {
        let (directory, services, admin, library, music) = support::library("playback").await;
        let original = support::write_wav(&music.join("track.wav"), 0);
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
            directory,
            music,
            services,
            actor,
            token: session.token,
            library,
            recording,
            source,
            original,
        }
    }
}
