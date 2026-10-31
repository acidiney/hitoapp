use crate::support::{admin, call, services};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::artist_catalog_similarity_service::{Reply, SimilarityServer};
use hito_core::test_support::support::write_wav;
use hito_core::{InstallPlugin, MetadataProvider, OfferPluginService, UpdateMetadataSettings};
use http_body_util::BodyExt;
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use tower::ServiceExt;

const M1: &str = "11111111-1111-4111-8111-111111111111";
const M2: &str = "22222222-2222-4222-8222-222222222222";

fn tagged(path: &std::path::Path, offset: i16, title: &str, mbid: Option<&str>, genre: &str) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist(format!("Artist {title}"));
    tag.set_album(format!("Album {title}"));
    tag.set_genre(genre.into());
    if let Some(mbid) = mbid {
        tag.insert_text(ItemKey::MusicBrainzArtistId, mbid.into());
    }
    tag.save_to_path(path, Default::default()).unwrap();
}

async fn page(app: &Router, path: &str, token: &str, partial: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .uri(path)
        .header("x-inertia", "true")
        .header("authorization", format!("Bearer {token}"));
    if let Some(component) = partial {
        request = request
            .header(
                "x-inertia-partial-component",
                component.split(':').next().unwrap(),
            )
            .header(
                "x-inertia-partial-data",
                component.split(':').nth(1).unwrap(),
            );
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

struct Fixture {
    _dir: tempfile::TempDir,
    app: Router,
    hito: hito_core::Services,
    token: String,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    tagged(&music.join("a.wav"), 1, "Song A", Some(M1), "Fado");
    tagged(&music.join("b.wav"), 2, "Song B", Some(M2), "Fado; Jazz");
    tagged(&music.join("c.wav"), 3, "Song C", None, "Jazz");
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        Some(&token),
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
    call(
        &app,
        "POST",
        "/api/v1/artist-matching",
        json!({"request_id": uuid::Uuid::new_v4()}),
        Some(&token),
    )
    .await;
    Fixture {
        _dir: dir,
        app,
        hito,
        token,
    }
}

impl Fixture {
    async fn group(&self, mbid: &str) -> String {
        let (_, groups) = call(
            &self.app,
            "GET",
            "/api/v1/artist-identity-groups",
            Value::Null,
            Some(&self.token),
        )
        .await;
        groups["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|group| group["provider_artist_id"] == mbid)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[tokio::test]
async fn discover_page_lists_genre_tiles_and_radio_artists_and_loads_them_only_when_asked() {
    let fixture = fixture().await;

    let (status, full) = page(&fixture.app, "/discover", &fixture.token, None).await;
    assert_eq!(status, StatusCode::OK);
    let props = &full["props"];
    let tiles: Vec<_> = props["genres"]
        .as_array()
        .unwrap()
        .iter()
        .map(|genre| {
            (
                genre["key"].as_str().unwrap(),
                genre["track_count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(tiles, [("fado", 2), ("jazz", 2)]);
    assert_eq!(props["artists"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(props["artists"]["limit"], 6);
    assert!(props["connections"].is_array());

    let (_, partial) = page(
        &fixture.app,
        "/discover",
        &fixture.token,
        Some("DiscoverPage:connections"),
    )
    .await;
    assert!(partial["props"].get("genres").is_none());
    assert!(partial["props"].get("artists").is_none());
}

#[tokio::test]
async fn genre_page_lists_the_albums_and_tracks_of_one_genre() {
    let fixture = fixture().await;

    let (status, found) = page(&fixture.app, "/genres?genre=FADO", &fixture.token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["component"], "GenrePage");
    let genre = &found["props"]["genre"];
    assert_eq!(genre["genre"]["label"], "Fado");
    assert_eq!(genre["genre"]["track_count"], 2);
    assert_eq!(genre["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(genre["albums"].as_array().unwrap().len(), 2);

    let (status, _) = page(&fixture.app, "/genres?genre=blues", &fixture.token, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = page(&fixture.app, "/genres", &fixture.token, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn radio_starts_from_a_chosen_artist_and_uses_the_similarity_provider_when_one_answers() {
    let fixture = fixture().await;
    let m1 = fixture.group(M1).await;
    let path = format!("/api/v1/artist-identity-groups/{m1}/radio");

    assert_eq!(
        call(&fixture.app, "POST", &path, Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, alone) = call(
        &fixture.app,
        "POST",
        &path,
        Value::Null,
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alone["similar_artists"], 0);
    let titles: Vec<_> = alone["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Song A"]);

    let mut server = SimilarityServer::start().await;
    let actor = fixture
        .hito
        .identity
        .authenticate(&fixture.token)
        .await
        .unwrap();
    let service = fixture
        .hito
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned similarity".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = fixture
        .hito
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let settings = fixture
        .hito
        .artist_catalog
        .metadata_settings(&actor)
        .await
        .unwrap();
    fixture
        .hito
        .artist_catalog
        .set_metadata_settings(
            &actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![
                    MetadataProvider::BuiltinMusicBrainz,
                    MetadataProvider::HttpPlugin {
                        installation_id: installation.id,
                    },
                ],
            },
        )
        .await
        .unwrap();
    server.reply(Reply::Similar(vec![(M2.into(), "Other".into(), Some(0.5))]));

    let (status, mixed) = call(
        &fixture.app,
        "POST",
        &path,
        Value::Null,
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mixed["similar_artists"], 1);
    let mut titles: Vec<_> = mixed["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Song A", "Song B"]);
    assert_eq!(
        server.similarity_requests(),
        vec![json!({"artist_id": M1, "limit": 25})]
    );

    let unknown = "/api/v1/artist-identity-groups/nope/radio";
    assert_eq!(
        call(
            &fixture.app,
            "POST",
            unknown,
            Value::Null,
            Some(&fixture.token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn discover_and_genre_pages_use_musicbrainz_genres_for_tracks_without_a_genre_tag() {
    use hito_core::test_support::musicbrainz::{FakeMusicBrainz, Reply};

    let musicbrainz = FakeMusicBrainz::start().await;
    musicbrainz.genres_of(
        "artist",
        M1,
        Reply::Genres(vec![("fado".into(), 4), ("folk".into(), 1)]),
    );
    musicbrainz.genres_of("artist", M2, Reply::Genres(vec![("jazz".into(), 2)]));
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    for (name, offset, mbid) in [("a", 1, M1), ("b", 2, M1), ("c", 3, M2)] {
        write_wav(&music.join(format!("{name}.wav")), offset);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("Song {name}"));
        tag.set_artist(format!("Artist {name}"));
        tag.set_album(format!("Album {name}"));
        tag.insert_text(ItemKey::MusicBrainzArtistId, mbid.into());
        tag.save_to_path(music.join(format!("{name}.wav")), Default::default())
            .unwrap();
    }
    let mut config = hito_core::CoreConfig::new(
        dir.path().join("hito.db"),
        music,
        hito_core::HostDeviceId::new("api-test-host").unwrap(),
    );
    config.musicbrainz_loopback_endpoint = Some(musicbrainz.endpoint.clone());
    let hito = hito_core::open(config).await.unwrap();
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, Some(&token)).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        Some(&token),
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
    let (_, before) = page(&app, "/discover", &token, None).await;
    assert!(before["props"]["genres"].as_array().unwrap().is_empty());
    while hito
        .artist_catalog
        .run_next_automatic_matching_until(std::future::pending())
        .await
        .unwrap()
    {}

    let (status, full) = page(&app, "/discover", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    let tiles: Vec<_> = full["props"]["genres"]
        .as_array()
        .unwrap()
        .iter()
        .map(|genre| {
            (
                genre["key"].as_str().unwrap(),
                genre["track_count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(tiles, [("fado", 2), ("folk", 2), ("jazz", 1)]);
    let (status, found) = page(&app, "/genres?genre=fado", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["props"]["genre"]["genre"]["track_count"], 2);
    assert_eq!(
        found["props"]["genre"]["tracks"].as_array().unwrap().len(),
        2
    );
}
