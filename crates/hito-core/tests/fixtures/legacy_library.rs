use hito_core::{Actor, CoreConfig, HostDeviceId, Services};
use sha2::{Digest, Sha256};
use sqlx::{migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{borrow::Cow, path::PathBuf};

pub const RECORDING: &str = "10000000-0000-4000-8000-000000000001";
pub const SOURCE: &str = "10000000-0000-4000-8000-000000000002";
pub const LIBRARY: &str = "10000000-0000-4000-8000-000000000003";
pub const PLAYLIST: &str = "10000000-0000-4000-8000-000000000004";
pub const ENTRY: &str = "10000000-0000-4000-8000-000000000005";

pub struct Legacy {
    pub directory: tempfile::TempDir,
    pub music: PathBuf,
    pub original: PathBuf,
}

impl Legacy {
    pub async fn seed() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original = music.join("z-original.wav");
        let bytes = crate::support::write_wav(&original, 0);
        let pool = sqlx::SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(directory.path().join("hito.db"))
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
        let all = sqlx::migrate!("./migrations");
        let legacy = Migrator {
            migrations: Cow::Owned(all.iter().filter(|m| m.version <= 6).cloned().collect()),
            ..Migrator::DEFAULT
        };
        legacy.run(&pool).await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO users(id,username,password_hash,admin) VALUES ('legacy-admin','admin','unused',1);
             INSERT INTO libraries VALUES ('10000000-0000-4000-8000-000000000003','Legacy library');
             INSERT INTO library_members VALUES ('10000000-0000-4000-8000-000000000003','legacy-admin');",
        ).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO sessions VALUES (?, 'legacy-admin', unixepoch() + 86400)")
            .bind(format!("{:x}", Sha256::digest(b"legacy-fixture-token")))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO recordings(id,library_id,title,artist,duration_ms,content_hash) VALUES (?,?,'Legacy track','Legacy artist',1000,?)")
            .bind(RECORDING).bind(LIBRARY).bind(format!("{:x}", Sha256::digest(bytes))).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,device_id) VALUES (?,?,'local',?,'server')")
            .bind(SOURCE).bind(RECORDING).bind(SOURCE).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO local_files(source_id,library_id,path,scan_root,size_bytes) VALUES (?,?,?,?,16044)")
            .bind(SOURCE).bind(LIBRARY).bind(original.to_string_lossy().as_ref()).bind(music.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO playlists(id,user_id,name) VALUES (?,'legacy-admin','Legacy playlist')",
        )
        .bind(PLAYLIST)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO playlist_entries(playlist_id,id,recording_id) VALUES (?,?,?)")
            .bind(PLAYLIST)
            .bind(ENTRY)
            .bind(RECORDING)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO likes VALUES ('legacy-admin',?,1)")
            .bind(RECORDING)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO listening_progress(user_id,recording_id,position_ms,speed) VALUES ('legacy-admin',?,500,1.25)")
            .bind(RECORDING).execute(&pool).await.unwrap();
        pool.close().await;
        Self {
            directory,
            music,
            original,
        }
    }

    pub async fn open(&self) -> (Services, Actor) {
        let services = hito_core::open(CoreConfig::new(
            self.directory.path().join("hito.db"),
            self.music.clone(),
            HostDeviceId::new("server").unwrap(),
        ))
        .await
        .unwrap();
        let actor = services
            .identity
            .authenticate("legacy-fixture-token")
            .await
            .unwrap();
        (services, actor)
    }
}
