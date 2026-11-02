use hito_core::test_support::collection_artwork::ArtworkServer;
use hito_core::test_support::support;
use hito_core::{
    Actor, Browse, CoverArtKind, CoverArtLookup, Credentials, ImportProviderPlaylist, Services,
    UpdateAutomaticLookups,
};
use uuid::Uuid;

pub struct Credited {
    pub hito: Services,
    pub actor: Actor,
    pub provider: ArtworkServer,
    pub artist: String,
    database: std::path::PathBuf,
    _directory: tempfile::TempDir,
}

pub async fn imported(provider: ArtworkServer) -> Credited {
    let (directory, hito, actor, _library, _music) = support::library("credited-photos").await;
    let installed = provider.connect(&hito, &actor).await;
    hito.collection
        .import_provider_playlist(
            &actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
            },
        )
        .await
        .unwrap();
    let artists = hito
        .catalog
        .artists(&actor, Browse::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    Credited {
        artist: artists.items[0].id.clone(),
        database: directory.path().join("hito.db"),
        hito,
        actor,
        provider,
        _directory: directory,
    }
}

impl Credited {
    pub async fn photo(&self) -> CoverArtLookup {
        self.hito
            .catalog
            .cover_art(&self.actor, CoverArtKind::CreditedArtist, &self.artist)
            .await
            .unwrap()
    }

    pub async fn drive(&self) -> usize {
        let mut steps = 0;
        while self
            .hito
            .catalog
            .run_next_cover_art_lookup_until(std::future::pending())
            .await
            .unwrap()
        {
            steps += 1;
            assert!(steps < 10);
        }
        steps
    }

    pub async fn provider_photo_offered(&self) -> bool {
        self.hito
            .catalog
            .artists(&self.actor, Browse::default())
            .await
            .unwrap()
            .items[0]
            .provider_photo
    }

    pub async fn run_unchecked(&self, statements: &[String]) {
        use sqlx::Connection;
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&self.database)
            .foreign_keys(false);
        let mut connection = sqlx::SqliteConnection::connect_with(&options)
            .await
            .unwrap();
        for statement in statements {
            sqlx::query(statement)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
    }

    pub async fn choose_musicbrainz_identity(&self) {
        let recording: String = {
            use sqlx::Connection;
            let mut connection =
                sqlx::SqliteConnection::connect(&format!("sqlite://{}", self.database.display()))
                    .await
                    .unwrap();
            let id = sqlx::query_scalar("SELECT recording_id FROM account_recording_credits")
                .fetch_one(&mut connection)
                .await
                .unwrap();
            connection.close().await.unwrap();
            id
        };
        let account = "(SELECT id FROM users)";
        self.run_unchecked(&[
            format!("INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,provider_artist_id) VALUES('group',{account},'builtin_musicbrainz','{MUSICBRAINZ_ARTIST}')"),
            format!("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) VALUES({account},'{recording}',1,'association','group','{artist}','observation',1)", artist = self.artist),
        ])
        .await;
    }

    pub async fn scalar(&self, query: &str) -> i64 {
        use sqlx::Connection;
        let mut connection =
            sqlx::SqliteConnection::connect(&format!("sqlite://{}", self.database.display()))
                .await
                .unwrap();
        let value = sqlx::query_scalar(query)
            .fetch_one(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        value
    }

    pub async fn queue_unrelated_lookups(&self, kind: &str, count: usize) {
        let statements: Vec<String> = (0..count)
            .map(|index| {
                let identifier = if kind == "credited_artist" {
                    format!("unrelated{index:022}")
                } else {
                    format!("0f800000-0000-4000-8000-{index:012}")
                };
                format!(
                    "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES((SELECT id FROM users),'{kind}','{identifier}','queued')"
                )
            })
            .collect();
        self.run_unchecked(&statements).await;
    }

    pub async fn another_account(&self) -> Actor {
        let credentials = || Credentials {
            username: "stranger".into(),
            password: "another account password".into(),
        };
        self.hito
            .identity
            .create_user(&self.actor, credentials())
            .await
            .unwrap();
        let session = self.hito.identity.login(credentials()).await.unwrap();
        self.hito
            .identity
            .authenticate(&session.token)
            .await
            .unwrap()
    }

    pub async fn automatic_artwork(&self, on: bool) {
        self.hito
            .artist_catalog
            .set_automatic_lookups(
                &self.actor,
                UpdateAutomaticLookups {
                    automatic_artist_lookup: None,
                    automatic_artwork: Some(on),
                    automatic_service_search: None,
                },
            )
            .await
            .unwrap();
    }
}

pub const MUSICBRAINZ_ARTIST: &str = "0f800000-0000-4000-8000-0000000000a1";
