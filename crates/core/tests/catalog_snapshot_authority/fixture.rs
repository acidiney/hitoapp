use hito_core::{Actor, AuthSession, CoreConfig, Credentials, HostDeviceId, Services};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use std::path::{Path, PathBuf};

pub const PASSWORD: &str = "snapshot authority test password";

pub struct Fixture {
    pub services: Services,
    pub admin: Actor,
    pub library: String,
    pub music: PathBuf,
    pub _directory: tempfile::TempDir,
}

impl Fixture {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let services = hito_core::open(CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            HostDeviceId::new("snapshot-authority-host").unwrap(),
        ))
        .await
        .unwrap();
        let session = services.identity.setup(credentials("admin")).await.unwrap();
        let admin = services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        let library = services
            .catalog
            .libraries(&admin)
            .await
            .unwrap()
            .remove(0)
            .id;
        Self {
            services,
            admin,
            library,
            music,
            _directory: directory,
        }
    }

    pub async fn account(&self, username: &str) -> (AuthSession, Actor) {
        let user = self
            .services
            .identity
            .create_user(&self.admin, credentials(username))
            .await
            .unwrap();
        self.services
            .identity
            .set_library_access(&self.admin, &self.library, &user.id, true)
            .await
            .unwrap();
        self.login(username).await
    }

    pub async fn login(&self, username: &str) -> (AuthSession, Actor) {
        let session = self
            .services
            .identity
            .login(credentials(username))
            .await
            .unwrap();
        let actor = self
            .services
            .identity
            .authenticate(&session.token)
            .await
            .unwrap();
        (session, actor)
    }

    pub async fn scan(&self) {
        let job = self
            .services
            .storage
            .enqueue_scan(&self.admin, &self.library)
            .await
            .unwrap()
            .remove(0);
        assert!(self.services.storage.run_next_job().await.unwrap());
        assert_eq!(
            self.services
                .storage
                .job(&self.admin, &job.id)
                .await
                .unwrap()
                .state,
            "complete"
        );
    }
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

pub fn write_wav(path: &Path) {
    let sample_count = 8_000u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + sample_count * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8_000u32.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(sample_count * 2).to_le_bytes());
    for sample in 0..sample_count {
        bytes.extend_from_slice(&((sample % 100) as i16 * 100).to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

pub fn tag_wav(path: &Path, title: &str, artist: &str, album: &str, disc: u32, track: u32) {
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(artist.into());
    tag.set_album(album.into());
    tag.insert_text(ItemKey::AlbumArtist, "Release performer".into());
    tag.set_disk(disc);
    tag.set_track(track);
    tag.save_to_path(path, Default::default()).unwrap();
}
