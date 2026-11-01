use hito_core::test_support::artist_images::{FakeArtistImages, Photo};
use hito_core::test_support::covers::COVER_A;
use hito_core::test_support::musicbrainz::{FakeMusicBrainz, Reply};
use hito_core::test_support::{playlist_service as fixture, support};
use hito_core::{
    Actor, ArtistCandidateSearchAvailability, CoverArtKind, CoverArtLookup, ImportProviderPlaylist,
    MetadataProvider, Services, UpdateAutomaticLookups,
};
use hito_plugin_sdk::ArtistSearchRequest;
use serde_json::json;
use uuid::Uuid;

const ARTIST: &str = "0f800000-0000-4000-8000-0000000000a1";
const SECOND: &str = "0f800000-0000-4000-8000-0000000000a2";

struct Photos {
    hito: Services,
    actor: Actor,
    musicbrainz: FakeMusicBrainz,
    images: FakeArtistImages,
    recording: String,
    database: std::path::PathBuf,
    _directory: tempfile::TempDir,
    _server: fixture::ProviderServer,
}

impl Photos {
    async fn prepare() -> Self {
        let musicbrainz = FakeMusicBrainz::start().await;
        let images = FakeArtistImages::start().await;
        let (directory, hito, actor, _library, _music) = support::library_with_artist_images(
            "artist-photos",
            Some(musicbrainz.endpoint.clone()),
            None,
            Some(images.endpoint.clone()),
        )
        .await;
        let server = fixture::ProviderServer::start().await;
        let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
        fixture::playlist(
            &server,
            "playlist/photos",
            "v1",
            vec![
                json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
                "id": "one", "title": "One", "artist": "Band", "duration_ms": 100000,
                "release": null, "disc_number": null, "track_number": null, "revision": "v1"}}}),
            ],
        )
        .await;
        let receipt = hito
            .collection
            .import_provider_playlist(
                &actor,
                &installed.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installed.revision,
                    playlist_reference: "https://provider.invalid/playlist/photos".into(),
                },
            )
            .await
            .unwrap();
        let playlist = hito
            .collection
            .playlist(&actor, &receipt.playlist_id.to_string())
            .await
            .unwrap();
        let recording = playlist.entries[0].recording_id.clone().unwrap();
        Self {
            hito,
            actor,
            musicbrainz,
            images,
            recording,
            database: directory.path().join("hito.db"),
            _directory: directory,
            _server: server,
        }
    }

    async fn search(&self) -> hito_plugin_sdk::ArtistSearchPage {
        let context = self
            .hito
            .artist_catalog
            .begin_artist_review(
                &self.actor,
                &self.recording,
                MetadataProvider::BuiltinMusicBrainz,
            )
            .await
            .unwrap();
        let found = self
            .hito
            .artist_catalog
            .search_artist_candidates_until(
                &self.actor,
                &context,
                ArtistSearchRequest {
                    query: "Band".into(),
                    cursor: None,
                    snapshot: None,
                    limit: 20,
                },
                std::future::pending(),
            )
            .await
            .unwrap();
        let ArtistCandidateSearchAvailability::Available { page, .. } = found else {
            panic!("the built-in search must answer");
        };
        page
    }

    async fn photo(&self, artist: &str) -> CoverArtLookup {
        self.hito
            .catalog
            .cover_art(&self.actor, CoverArtKind::Artist, artist)
            .await
            .unwrap()
    }

    async fn drive(&self) {
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
    }

    async fn age_lookups(&self) {
        use sqlx::Connection;
        let mut connection =
            sqlx::SqliteConnection::connect(&format!("sqlite://{}", self.database.display()))
                .await
                .unwrap();
        sqlx::query("UPDATE cover_art_lookups SET updated_at = unixepoch() - 90000")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    }

    fn link(&self, artist: &str, entity: &str, file: &str, photo: Photo) {
        self.musicbrainz.links_of(artist, Some(entity));
        self.images.entity(entity, Some(file));
        self.images.photo(&file.replace(' ', "_"), photo);
    }
}

fn is_ready(lookup: &CoverArtLookup) -> bool {
    matches!(lookup, CoverArtLookup::Ready(_))
}

#[tokio::test]
async fn candidates_state_their_country_and_start_year_and_an_artist_photo_is_fetched_once_through_wikidata_and_commons()
 {
    let photos = Photos::prepare().await;
    photos.musicbrainz.search(
        "Band",
        Reply::Candidates(vec![(ARTIST.into(), "Band".into())]),
    );
    photos.musicbrainz.describe(ARTIST, "UK soul singer", "GB");
    photos.musicbrainz.began(ARTIST, "2008-04-20");
    photos.link(
        ARTIST,
        "Q42",
        "Band photo.png",
        Photo::Png(COVER_A.to_vec()),
    );

    let page = photos.search().await;
    assert_eq!(page.items[0].country.as_deref(), Some("GB"));
    assert_eq!(page.items[0].begin_year, Some(2008));

    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Pending
    ));
    photos.drive().await;
    let CoverArtLookup::Ready(image) = photos.photo(ARTIST).await else {
        panic!("the photo must be stored");
    };
    assert_eq!(image.content_type, "image/png");
    photos.drive().await;
    assert_eq!(
        photos.images.requests(),
        vec![
            "/wiki/Special:EntityData/Q42.json".to_owned(),
            "/wiki/Special:FilePath/Band_photo.png?width=320".to_owned(),
            "/thumb/Band_photo.png".to_owned(),
        ],
        "Only the fixed paths are requested, once"
    );
    assert!(
        photos
            .musicbrainz
            .requests()
            .iter()
            .any(|target| target.contains(&format!("artist/{ARTIST}"))
                && target.contains("inc=url-rels"))
    );
}

#[tokio::test]
async fn with_automatic_artwork_off_nothing_is_queued_or_fetched() {
    let photos = Photos::prepare().await;
    photos.link(
        ARTIST,
        "Q42",
        "Band photo.png",
        Photo::Png(COVER_A.to_vec()),
    );
    photos
        .hito
        .artist_catalog
        .set_automatic_lookups(
            &photos.actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: None,
                automatic_artwork: Some(false),
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Unknown
    ));
    assert!(
        !photos
            .hito
            .catalog
            .run_next_cover_art_lookup_until(std::future::pending())
            .await
            .unwrap()
    );
    assert!(photos.images.requests().is_empty());
    assert!(
        !photos
            .musicbrainz
            .requests()
            .iter()
            .any(|target| target.contains("url-rels"))
    );
}

#[tokio::test]
async fn an_artist_without_a_wikidata_link_or_an_image_claim_has_no_photo_and_asks_commons_nothing()
{
    let photos = Photos::prepare().await;
    photos.musicbrainz.links_of(ARTIST, None);
    photos.musicbrainz.links_of(SECOND, Some("Q7"));
    photos.images.entity("Q7", None);
    for artist in [ARTIST, SECOND] {
        assert!(matches!(
            photos.photo(artist).await,
            CoverArtLookup::Pending
        ));
    }
    photos.drive().await;
    for artist in [ARTIST, SECOND] {
        assert!(matches!(
            photos.photo(artist).await,
            CoverArtLookup::Missing
        ));
    }
    assert_eq!(
        photos.images.requests(),
        vec!["/wiki/Special:EntityData/Q7.json".to_owned()]
    );
}

#[tokio::test]
async fn a_photo_that_is_not_a_real_image_or_leaves_the_allowed_hosts_is_never_stored() {
    let photos = Photos::prepare().await;
    let cases = [
        ("Html.png", Photo::Html),
        (
            "Fake.png",
            Photo::Mislabelled(b"not an image at all".to_vec()),
        ),
        (
            "Away.png",
            Photo::Redirect("http://127.0.0.1:9/elsewhere.png".into()),
        ),
        ("Down.png", Photo::Unavailable),
        ("Huge.png", Photo::Mislabelled(vec![0x89; 3 * 1024 * 1024])),
    ];
    let artists: Vec<String> = (0..cases.len())
        .map(|index| format!("0f800000-0000-4000-8000-0000000001{index:02}"))
        .collect();
    for (index, ((file, photo), artist)) in cases.into_iter().zip(&artists).enumerate() {
        photos.link(artist, &format!("Q{}", index + 100), file, photo);
        assert!(matches!(
            photos.photo(artist).await,
            CoverArtLookup::Pending
        ));
    }
    photos.drive().await;
    for artist in &artists {
        assert!(!is_ready(&photos.photo(artist).await), "{artist}");
    }
}

#[tokio::test]
async fn an_artist_without_a_photo_is_asked_again_only_after_a_day_and_then_gets_one() {
    let photos = Photos::prepare().await;
    photos.musicbrainz.links_of(ARTIST, None);
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Pending
    ));
    photos.drive().await;
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Missing
    ));
    photos.link(
        ARTIST,
        "Q42",
        "Band photo.png",
        Photo::Png(COVER_A.to_vec()),
    );
    photos.drive().await;
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Missing
    ));
    photos.age_lookups().await;
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Pending
    ));
    photos.drive().await;
    assert!(is_ready(&photos.photo(ARTIST).await));
}

#[tokio::test]
async fn a_stored_photo_is_not_served_once_automatic_artwork_is_off() {
    let photos = Photos::prepare().await;
    photos.link(
        ARTIST,
        "Q42",
        "Band photo.png",
        Photo::Png(COVER_A.to_vec()),
    );
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Pending
    ));
    photos.drive().await;
    assert!(is_ready(&photos.photo(ARTIST).await));
    photos
        .hito
        .artist_catalog
        .set_automatic_lookups(
            &photos.actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: None,
                automatic_artwork: Some(false),
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        photos.photo(ARTIST).await,
        CoverArtLookup::Unknown
    ));
}
