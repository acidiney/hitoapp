use hito_core::test_support::podcast_audio_service::PodcastAudioServer;
use hito_core::test_support::support;
use hito_core::{
    Action, Actor, AuthSession, CoreConfig, Credentials, DownloadRequest, DownloadState,
    HostDeviceId, PodcastEpisodeQuery, PodcastNetworkConfig, Services, SubscribePodcast, open,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{future::pending, path::Path};
use uuid::Uuid;

pub const PASSWORD: &str = "a long account deletion password";

pub fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

pub fn config(directory: &Path, origin: &str) -> CoreConfig {
    let mut config = CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("account-deletion").unwrap(),
    );
    config.podcast_network = PodcastNetworkConfig::new(vec![origin.into()]).unwrap();
    config
}

pub async fn connect(directory: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(directory.join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

pub struct World {
    pub directory: tempfile::TempDir,
    pub hito: Services,
    pub server: PodcastAudioServer,
    pub admin: AuthSession,
    pub member: AuthSession,
    pub shared_recording: Uuid,
}

pub struct Footprint {
    pub recording: Uuid,
    pub files: Vec<String>,
}

impl World {
    pub async fn empty() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("music")).unwrap();
        support::write_wav(&directory.path().join("music/shared.wav"), 11);
        let wav = support::write_wav(&directory.path().join("episode.wav"), 149);
        let server = PodcastAudioServer::start(wav).await;
        let hito = open(config(directory.path(), &server.origin))
            .await
            .unwrap();
        let admin = hito.identity.setup(credentials("admin")).await.unwrap();
        let actor = hito.identity.authenticate(&admin.token).await.unwrap();
        hito.identity
            .create_user(&actor, credentials("erasable-person"))
            .await
            .unwrap();
        let member = hito
            .identity
            .login(credentials("erasable-person"))
            .await
            .unwrap();
        let library = hito.catalog.libraries(&actor).await.unwrap().remove(0).id;
        hito.identity
            .set_library_access(&actor, &library, &member.user.id, true)
            .await
            .unwrap();
        support::scan(&hito, &actor, &library).await;
        let mut connection = connect(directory.path()).await;
        let shared: String = sqlx::query_scalar("SELECT r.id FROM recordings r JOIN libraries l ON l.id=r.library_id WHERE l.owner_id IS NULL LIMIT 1")
            .fetch_one(&mut connection)
            .await
            .unwrap();
        Self {
            directory,
            hito,
            server,
            admin,
            member,
            shared_recording: Uuid::parse_str(&shared).unwrap(),
        }
    }

    pub async fn actor(&self, session: &AuthSession) -> Actor {
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    pub async fn populate(&self, session: &AuthSession) -> Footprint {
        let actor = self.actor(session).await;
        let subscription = self
            .hito
            .podcasts
            .subscribe(
                &actor,
                SubscribePodcast {
                    request_id: Uuid::new_v4(),
                    feed_url: self.server.feed_url.clone(),
                },
            )
            .await
            .unwrap();
        assert!(
            self.hito
                .podcasts
                .run_next_refresh_until(pending())
                .await
                .unwrap()
        );
        let episodes = self
            .hito
            .podcasts
            .episodes(
                &actor,
                &subscription.subscription_id.to_string(),
                PodcastEpisodeQuery::default(),
            )
            .await
            .unwrap();
        let recording = Uuid::parse_str(&episodes.items[0].recording.id).unwrap();
        let playlist = Uuid::new_v4();
        support::synchronize(
            &self.hito,
            &actor,
            vec![
                Action::RecordingTitleSet {
                    recording_id: recording,
                    title: "Personal episode title".into(),
                },
                Action::LikeSet {
                    recording_id: recording,
                    liked: true,
                },
                Action::LikeSet {
                    recording_id: self.shared_recording,
                    liked: true,
                },
                Action::ProgressSet {
                    recording_id: self.shared_recording,
                    position_ms: 900,
                    speed: 1.0,
                },
                Action::PlaylistCreate {
                    playlist_id: playlist,
                    name: "Personal mix".into(),
                },
                Action::EntryInsert {
                    playlist_id: playlist,
                    entry_id: Uuid::new_v4(),
                    recording_id: self.shared_recording,
                    position: 0,
                },
            ],
        )
        .await;
        self.hito
            .downloads
            .create(
                &actor,
                &recording.to_string(),
                DownloadRequest {
                    request_id: Uuid::new_v4(),
                },
            )
            .await
            .unwrap();
        assert!(
            self.hito
                .downloads
                .run_next_job_until(pending())
                .await
                .unwrap()
        );
        let status = self
            .hito
            .downloads
            .status(&actor, &recording.to_string())
            .await
            .unwrap();
        assert!(matches!(status.status, DownloadState::Ready));
        let mut connection = connect(self.directory.path()).await;
        let files: Vec<String> = sqlx::query_scalar(
            "SELECT id||'.'||attempt_id||'.media' FROM download_files WHERE account_id=? AND state='ready'",
        )
        .bind(&session.user.id)
        .fetch_all(&mut connection)
        .await
        .unwrap();
        assert_eq!(files.len(), 1);
        Footprint { recording, files }
    }

    pub fn file_exists(&self, name: &str) -> bool {
        self.directory
            .path()
            .join("hito.downloads")
            .join(name)
            .exists()
    }
}

/// Every table column that names an account, as table and column.
pub async fn account_columns(connection: &mut SqliteConnection) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT m.name,p.name FROM sqlite_master m, pragma_table_info(m.name) p
         WHERE m.type='table' AND m.name NOT LIKE '%fts%' AND m.name NOT LIKE 'sqlite_%'
         AND p.name IN ('user_id','account_id','owner_id') ORDER BY 1,2",
    )
    .fetch_all(connection)
    .await
    .unwrap()
}

pub async fn rows_of(connection: &mut SqliteConnection, account: &str) -> Vec<(String, i64)> {
    let mut rows = Vec::new();
    for (table, column) in account_columns(connection).await {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE {column}=?"))
                .bind(account)
                .fetch_one(&mut *connection)
                .await
                .unwrap();
        rows.push((format!("{table}.{column}"), count));
    }
    rows
}

/// Tables and columns whose text mentions the account anywhere, including JSON payloads.
pub async fn mentions(connection: &mut SqliteConnection, account: &str) -> Vec<String> {
    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT m.name,p.name FROM sqlite_master m, pragma_table_info(m.name) p
         WHERE m.type='table' AND m.name NOT LIKE '%fts%' AND m.name NOT LIKE 'sqlite_%'
         AND m.name<>'_sqlx_migrations' AND p.type IN ('TEXT','')",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    let mut found = Vec::new();
    for (table, column) in columns {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM \"{table}\" WHERE instr(\"{column}\", ?)>0"
        ))
        .bind(account)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        if count > 0 {
            found.push(format!("{table}.{column}"));
        }
    }
    found
}
