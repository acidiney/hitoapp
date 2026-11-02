use hito_core::test_support::collection_artwork::{
    ARTIST_ID, ARTIST_PHOTO, ArtistKind, ArtworkServer,
};
use hito_core::test_support::support;
use hito_core::{
    Actor, Browse, CoverArtKind, CoverArtLookup, ImportProviderPlaylist, Services,
    UpdateAutomaticLookups,
};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use uuid::Uuid;

struct Credited {
    hito: Services,
    actor: Actor,
    provider: ArtworkServer,
    artist: String,
    database: std::path::PathBuf,
    _directory: tempfile::TempDir,
}

async fn imported(provider: ArtworkServer) -> Credited {
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
    async fn photo(&self) -> CoverArtLookup {
        self.hito
            .catalog
            .cover_art(&self.actor, CoverArtKind::CreditedArtist, &self.artist)
            .await
            .unwrap()
    }

    async fn drive(&self) -> usize {
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

    async fn provider_photo_offered(&self) -> bool {
        self.hito
            .catalog
            .artists(&self.actor, Browse::default())
            .await
            .unwrap()
            .items[0]
            .provider_photo
    }

    async fn run_unchecked(&self, statements: &[String]) {
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

    async fn choose_musicbrainz_identity(&self) {
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

    async fn automatic_artwork(&self, on: bool) {
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

const MUSICBRAINZ_ARTIST: &str = "0f800000-0000-4000-8000-0000000000a1";

fn artist_request() -> ArtworkRequest {
    ArtworkRequest {
        entity: ArtworkEntity::Artist,
        provider_id: ARTIST_ID.into(),
    }
}

#[tokio::test]
async fn a_credit_known_only_through_a_provider_gets_its_photo_from_the_plugin_once() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    assert!(t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert_eq!(t.drive().await, 1);
    let CoverArtLookup::Ready(image) = t.photo().await else {
        panic!("the photo must be stored");
    };
    assert_eq!(image.bytes, ARTIST_PHOTO);
    assert_eq!(t.drive().await, 0);
    assert_eq!(t.provider.requests().await, [artist_request()]);
}

#[tokio::test]
async fn a_credit_without_a_provider_artist_id_is_never_queued_or_requested() {
    let t = imported(ArtworkServer::start().await).await;
    assert!(!t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
}

#[tokio::test]
async fn with_automatic_artwork_off_nothing_is_queued_requested_or_served() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    t.automatic_artwork(false).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
    t.automatic_artwork(true).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    t.drive().await;
    assert!(matches!(t.photo().await, CoverArtLookup::Ready(_)));
    t.automatic_artwork(false).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
}

#[tokio::test]
async fn a_plugin_that_does_not_know_the_artist_kind_leaves_the_credit_without_a_photo() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Refused).await).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert_eq!(t.drive().await, 1);
    assert!(matches!(t.photo().await, CoverArtLookup::Missing));
    assert_eq!(t.drive().await, 0);
    assert_eq!(t.provider.requests().await, [artist_request()]);
}

#[tokio::test]
async fn an_artist_identifier_that_is_not_an_owned_credit_is_refused() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    for artist in ["", "not an id", &"a".repeat(65)] {
        assert!(
            t.hito
                .catalog
                .cover_art(&t.actor, CoverArtKind::CreditedArtist, artist)
                .await
                .is_err()
        );
    }
    let stranger = "f".repeat(32);
    assert!(matches!(
        t.hito
            .catalog
            .cover_art(&t.actor, CoverArtKind::CreditedArtist, &stranger)
            .await
            .unwrap(),
        CoverArtLookup::Unknown
    ));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
}

#[tokio::test]
async fn a_chosen_musicbrainz_identity_keeps_the_photo_until_wikidata_has_none() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    t.choose_musicbrainz_identity().await;
    assert!(!t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
    let account = "(SELECT id FROM users)";
    t.run_unchecked(&[format!(
        "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES({account},'artist','{MUSICBRAINZ_ARTIST}','missing')"
    )])
    .await;
    assert!(t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert_eq!(t.drive().await, 1);
    assert!(matches!(t.photo().await, CoverArtLookup::Ready(_)));
    assert_eq!(t.provider.requests().await, [artist_request()]);
}
