use hito_core::test_support::cover_art::{FakeCoverArt, Reply};
use hito_core::test_support::covers::COVER_A;
use hito_core::test_support::musicbrainz::FakeMusicBrainz;
use hito_core::test_support::{playlist_service as fixture, support};
use hito_core::{
    Actor, Browse, CoverArtKind, CoverArtLookup, ImportProviderPlaylist, Services,
    UpdateAutomaticLookups,
};
use serde_json::json;
use sqlx::Connection;
use uuid::Uuid;

const RELEASE_MBID: &str = "0f700000-0000-4000-8000-0000000000a1";
const OTHER_MBID: &str = "0f700000-0000-4000-8000-0000000000a2";
const GROUP_MBID: &str = "0f700000-0000-4000-8000-0000000000a3";

struct Albums {
    hito: Services,
    actor: Actor,
    caa: FakeCoverArt,
    _musicbrainz: FakeMusicBrainz,
    releases: Vec<String>,
    database: std::path::PathBuf,
    _directory: tempfile::TempDir,
    _server: fixture::ProviderServer,
}

fn track(id: &str, album: &str) -> serde_json::Value {
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": id, "title": id, "artist": "Band", "duration_ms": 100000,
        "release": {"id": album, "title": album}, "disc_number": 1, "track_number": 1,
        "revision": "v1"}}})
}

impl Albums {
    async fn prepare() -> Self {
        let caa = FakeCoverArt::start().await;
        let musicbrainz = FakeMusicBrainz::start().await;
        let (directory, hito, actor, _library, _music) = support::library_with_services(
            "cover-art",
            Some(musicbrainz.endpoint.clone()),
            Some(caa.endpoint.clone()),
        )
        .await;
        let server = fixture::ProviderServer::start().await;
        let installed = fixture::connect(&hito, &actor, &server, &["playlists"]).await;
        fixture::playlist(
            &server,
            "playlist/covers",
            "v1",
            vec![track("one", "First"), track("two", "Second")],
        )
        .await;
        hito.collection
            .import_provider_playlist(
                &actor,
                &installed.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installed.revision,
                    playlist_reference: "https://provider.invalid/playlist/covers".into(),
                },
            )
            .await
            .unwrap();
        let mut albums = hito
            .catalog
            .releases(&actor, Browse::default())
            .await
            .unwrap()
            .items;
        albums.sort_by(|a, b| a.title.cmp(&b.title));
        let database = directory.path().join("hito.db");
        let me = Self {
            hito,
            actor,
            caa,
            _musicbrainz: musicbrainz,
            releases: albums.into_iter().map(|a| a.id).collect(),
            database,
            _directory: directory,
            _server: server,
        };
        me.tag(&me.releases[0], "release", RELEASE_MBID).await;
        me.tag(&me.releases[1], "release", OTHER_MBID).await;
        me
    }

    async fn tag(&self, release: &str, entity: &str, mbid: &str) {
        let mut connection =
            sqlx::SqliteConnection::connect(&format!("sqlite://{}", self.database.display()))
                .await
                .unwrap();
        sqlx::query("INSERT INTO recording_musicbrainz_observations(observation_id,entity,mbid,recording_id,release_id)
            SELECT r.metadata_observation_id,?,?,r.id,r.release_id FROM recordings r WHERE r.release_id = ?")
            .bind(entity).bind(mbid).bind(release)
            .execute(&mut connection)
            .await
            .expect("observation insert");
        connection.close().await.unwrap();
    }

    async fn state(&self, mbid: &str) -> Option<(String, i64)> {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=ro",
            self.database.display()
        ))
        .await
        .unwrap();
        sqlx::query_as("SELECT state,attempts FROM cover_art_lookups WHERE mbid = ?")
            .bind(mbid)
            .fetch_optional(&mut connection)
            .await
            .unwrap()
    }

    async fn age(&self, mbid: &str) {
        let mut connection =
            sqlx::SqliteConnection::connect(&format!("sqlite://{}", self.database.display()))
                .await
                .unwrap();
        sqlx::query("UPDATE cover_art_lookups SET updated_at = unixepoch() - 90000 WHERE mbid = ?")
            .bind(mbid)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
    }

    async fn fill_pending_lookups(&self, count: usize) {
        let mut connection =
            sqlx::SqliteConnection::connect(&format!("sqlite://{}", self.database.display()))
                .await
                .unwrap();
        for index in 0..count {
            sqlx::query(
                "INSERT INTO cover_art_lookups(user_id,kind,mbid,state)
                 SELECT DISTINCT user_id,'release',?,'queued' FROM library_members",
            )
            .bind(format!("0f700000-0000-4000-8000-{index:012}"))
            .execute(&mut connection)
            .await
            .unwrap();
        }
        connection.close().await.unwrap();
    }

    async fn pending_count(&self) -> i64 {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=ro",
            self.database.display()
        ))
        .await
        .unwrap();
        sqlx::query_scalar(
            "SELECT count(*) FROM cover_art_lookups WHERE state IN ('queued','running')",
        )
        .fetch_one(&mut connection)
        .await
        .unwrap()
    }

    async fn walk(&self) {
        let mut steps = 0;
        while self
            .hito
            .artist_catalog
            .run_next_automatic_matching_until(std::future::pending())
            .await
            .unwrap()
        {
            steps += 1;
            assert!(steps < 20);
        }
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
}

#[tokio::test]
async fn an_album_without_artwork_fetches_its_front_cover_once_and_keeps_it() {
    let albums = Albums::prepare().await;
    albums
        .caa
        .front("release", RELEASE_MBID, Reply::Png(COVER_A.to_vec()));
    let (hito, actor, release) = (&albums.hito, &albums.actor, &albums.releases[0]);

    assert!(
        hito.catalog
            .release_artwork(actor, release)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(albums.state(RELEASE_MBID).await.unwrap().0, "queued");
    albums.drive().await;
    let cover = hito
        .catalog
        .release_artwork(actor, release)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cover.content_type, "image/png");
    assert_eq!(cover.bytes, COVER_A);
    assert_eq!(albums.state(RELEASE_MBID).await.unwrap().0, "ready");
    assert!(
        hito.catalog
            .release_artwork(actor, release)
            .await
            .unwrap()
            .is_some()
    );
    albums.drive().await;
    assert_eq!(
        albums.caa.requests(),
        vec![format!("/release/{RELEASE_MBID}/front-500")],
        "Only the identifier is requested, once"
    );
    assert!(matches!(
        hito.catalog
            .cover_art(actor, CoverArtKind::Release, RELEASE_MBID)
            .await
            .unwrap(),
        CoverArtLookup::Ready(_)
    ));
}

#[tokio::test]
async fn an_absent_cover_is_reported_honestly_and_a_down_archive_is_retried_later() {
    let albums = Albums::prepare().await;
    albums.caa.front("release", RELEASE_MBID, Reply::NotFound);
    albums.caa.front("release", OTHER_MBID, Reply::Unavailable);
    let (hito, actor) = (&albums.hito, &albums.actor);
    for release in &albums.releases {
        assert!(
            hito.catalog
                .release_artwork(actor, release)
                .await
                .unwrap()
                .is_none()
        );
    }
    albums.drive().await;
    assert_eq!(albums.state(RELEASE_MBID).await.unwrap().0, "missing");
    let (state, attempts) = albums.state(OTHER_MBID).await.unwrap();
    assert_eq!(
        (state.as_str(), attempts),
        ("queued", 1),
        "Retried later, not forever"
    );
    assert!(matches!(
        hito.catalog
            .cover_art(actor, CoverArtKind::Release, RELEASE_MBID)
            .await
            .unwrap(),
        CoverArtLookup::Missing
    ));
    assert!(matches!(
        hito.catalog
            .cover_art(actor, CoverArtKind::Release, OTHER_MBID)
            .await
            .unwrap(),
        CoverArtLookup::Pending
    ));
    assert!(matches!(
        hito.catalog
            .cover_art(actor, CoverArtKind::ReleaseGroup, GROUP_MBID)
            .await
            .unwrap(),
        CoverArtLookup::Unknown
    ));
}

#[tokio::test]
async fn turning_automatic_artwork_off_sends_nothing() {
    let albums = Albums::prepare().await;
    albums
        .caa
        .front("release", RELEASE_MBID, Reply::Png(COVER_A.to_vec()));
    let (hito, actor) = (&albums.hito, &albums.actor);
    hito.artist_catalog
        .set_automatic_lookups(
            actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: None,
                automatic_artwork: Some(false),
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    assert!(
        hito.catalog
            .release_artwork(actor, &albums.releases[0])
            .await
            .unwrap()
            .is_none()
    );
    albums.drive().await;
    assert!(albums.caa.requests().is_empty());
    assert!(albums.state(RELEASE_MBID).await.is_none());
}

#[tokio::test]
async fn a_pending_lookup_is_dropped_when_artwork_is_turned_off_before_it_runs() {
    let albums = Albums::prepare().await;
    albums
        .caa
        .front("release", RELEASE_MBID, Reply::Png(COVER_A.to_vec()));
    let (hito, actor) = (&albums.hito, &albums.actor);
    hito.catalog
        .release_artwork(actor, &albums.releases[0])
        .await
        .unwrap();
    hito.artist_catalog
        .set_automatic_lookups(
            actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: None,
                automatic_artwork: Some(false),
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    albums.drive().await;
    assert!(albums.caa.requests().is_empty());
}

#[tokio::test]
async fn the_matching_walk_queues_covers_for_albums_without_local_files() {
    let albums = Albums::prepare().await;
    albums
        .caa
        .front("release", RELEASE_MBID, Reply::Png(COVER_A.to_vec()));
    albums.caa.front("release", OTHER_MBID, Reply::NotFound);
    let (hito, actor) = (&albums.hito, &albums.actor);
    let mut steps = 0;
    while hito
        .artist_catalog
        .run_next_automatic_matching_until(std::future::pending())
        .await
        .unwrap()
    {
        steps += 1;
        assert!(steps < 20);
    }
    assert_eq!(albums.state(RELEASE_MBID).await.unwrap().0, "queued");
    albums.drive().await;
    assert!(
        hito.catalog
            .release_artwork(actor, &albums.releases[0])
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        hito.catalog
            .release_artwork(actor, &albums.releases[1])
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(albums.state(OTHER_MBID).await.unwrap().0, "missing");
}

#[tokio::test]
async fn a_missing_cover_is_asked_again_after_the_retry_window_when_the_album_is_read() {
    let albums = Albums::prepare().await;
    albums.caa.front("release", RELEASE_MBID, Reply::NotFound);
    let (hito, actor, release) = (&albums.hito, &albums.actor, &albums.releases[0]);
    hito.catalog.release_artwork(actor, release).await.unwrap();
    albums.drive().await;
    assert_eq!(albums.state(RELEASE_MBID).await.unwrap().0, "missing");

    hito.catalog.release_artwork(actor, release).await.unwrap();
    assert_eq!(
        albums.state(RELEASE_MBID).await.unwrap().0,
        "missing",
        "A fresh answer is not asked for again"
    );
    albums.age(RELEASE_MBID).await;
    albums
        .caa
        .front("release", RELEASE_MBID, Reply::Png(COVER_A.to_vec()));
    hito.catalog.release_artwork(actor, release).await.unwrap();
    assert_eq!(albums.state(RELEASE_MBID).await.unwrap().0, "queued");
    albums.drive().await;
    assert!(
        hito.catalog
            .release_artwork(actor, release)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn the_matching_walk_queues_no_cover_while_artwork_is_off() {
    let albums = Albums::prepare().await;
    albums
        .hito
        .artist_catalog
        .set_automatic_lookups(
            &albums.actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: None,
                automatic_artwork: Some(false),
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
    albums.walk().await;
    assert!(albums.state(RELEASE_MBID).await.is_none());
    assert!(albums.state(OTHER_MBID).await.is_none());
}

#[tokio::test]
async fn the_matching_walk_queues_nothing_beyond_the_pending_cap() {
    let albums = Albums::prepare().await;
    albums.fill_pending_lookups(64).await;
    albums.walk().await;
    assert!(albums.state(RELEASE_MBID).await.is_none());
    assert!(albums.state(OTHER_MBID).await.is_none());
    assert_eq!(albums.pending_count().await, 64);
}

#[tokio::test]
async fn the_matching_walk_fills_only_the_room_left_under_the_pending_cap() {
    let albums = Albums::prepare().await;
    albums.fill_pending_lookups(63).await;
    albums.walk().await;
    assert_eq!(albums.pending_count().await, 64);
}
