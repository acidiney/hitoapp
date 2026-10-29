use crate::support::{admin, call, services};
use axum::http::StatusCode;
use hito_core::test_support::artist_catalog_similarity_service::{Reply, SimilarityServer};
use hito_core::test_support::support::write_wav;
use hito_core::{InstallPlugin, MetadataProvider, OfferPluginService, UpdateMetadataSettings};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};

const M1: &str = "11111111-1111-4111-8111-111111111111";
const M2: &str = "22222222-2222-4222-8222-222222222222";
const M3: &str = "33333333-3333-4333-8333-333333333333";

fn tagged(path: &std::path::Path, offset: i16, title: &str, mbid: &str) {
    write_wav(path, offset);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title(title.into());
    tag.set_artist("Owned artist".into());
    tag.set_album(format!("Album {title}"));
    tag.insert_text(ItemKey::MusicBrainzArtistId, mbid.into());
    tag.save_to_path(path, Default::default()).unwrap();
}

#[tokio::test]
async fn http_similar_artists_use_the_chosen_plugin_and_link_exact_chosen_artists() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("music");
    std::fs::create_dir(&music).unwrap();
    tagged(&music.join("a.wav"), 1, "Song A", M1);
    tagged(&music.join("b.wav"), 2, "Song B", M2);
    let hito = services(dir.path(), &music).await;
    let app = hito_server::router(hito.clone());
    let token = admin(&app).await;
    let token = Some(token.as_str());
    let (_, libraries) = call(&app, "GET", "/api/v1/libraries", Value::Null, token).await;
    let library = libraries[0]["id"].as_str().unwrap().to_owned();
    call(
        &app,
        "POST",
        &format!("/api/v1/libraries/{library}/scan"),
        Value::Null,
        token,
    )
    .await;
    hito.storage.run_next_job().await.unwrap();
    call(
        &app,
        "POST",
        "/api/v1/artist-matching",
        json!({"request_id": uuid::Uuid::new_v4()}),
        token,
    )
    .await;
    let (_, groups) = call(
        &app,
        "GET",
        "/api/v1/artist-identity-groups",
        Value::Null,
        token,
    )
    .await;
    let find = |mbid: &str| {
        groups["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|group| group["provider_artist_id"] == mbid)
            .unwrap()
            .clone()
    };
    let seed = find(M1);
    let path = format!(
        "/api/v1/recordings/{}/artist-associations/{}/similar-artists",
        seed["name"]["recording_id"].as_str().unwrap(),
        seed["name"]["association_id"].as_str().unwrap()
    );
    let body = json!({"limit": 5});

    assert_eq!(
        call(&app, "POST", &path, body.clone(), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, none) = call(&app, "POST", &path, body.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(none, json!({"status": "unsupported"}));

    let mut server = SimilarityServer::start().await;
    let admin_actor = hito.identity.authenticate(token.unwrap()).await.unwrap();
    let service = hito
        .plugins
        .offer_service(
            &admin_actor,
            OfferPluginService {
                name: "Owned similarity".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            &admin_actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let plugin = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let settings = hito.catalog.metadata_settings(&admin_actor).await.unwrap();
    hito.catalog
        .set_metadata_settings(
            &admin_actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![MetadataProvider::BuiltinMusicBrainz, plugin.clone()],
            },
        )
        .await
        .unwrap();
    server.reply(Reply::Similar(vec![
        (M2.into(), "Qualquer nome".into(), Some(0.75)),
        (M3.into(), "Owned artist".into(), None),
    ]));

    let (status, found) = call(&app, "POST", &path, body.clone(), token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["status"], "available");
    assert_eq!(found["provider"], serde_json::to_value(&plugin).unwrap());
    assert_eq!(found["association_id"], seed["name"]["association_id"]);
    assert_eq!(found["artists"][0]["provider_artist_id"], M2);
    assert_eq!(found["artists"][0]["chosen_artist_id"], find(M2)["id"]);
    assert_eq!(found["artists"][1]["chosen_artist_id"], Value::Null);
    assert_eq!(
        server.similarity_requests(),
        vec![json!({"artist_id": M1, "limit": 5})]
    );

    for bad in [
        json!({"limit": 0}),
        json!({"limit": 26}),
        json!({"limit": 5, "listens": []}),
        json!({}),
    ] {
        assert_eq!(
            call(&app, "POST", &path, bad, token).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    server.reply(Reply::Status(StatusCode::BAD_GATEWAY));
    let (status, down) = call(&app, "POST", &path, body.clone(), token).await;
    assert_eq!(
        (status, down),
        (StatusCode::OK, json!({"status": "unavailable"}))
    );
    let missing = path.replace(
        seed["name"]["association_id"].as_str().unwrap(),
        &uuid::Uuid::new_v4().to_string(),
    );
    assert_eq!(
        call(&app, "POST", &missing, body, token).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}
