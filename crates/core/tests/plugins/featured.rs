use axum::http::StatusCode;
use hito_core::test_support::featured_service::{Cover, FeaturedServer, Reply, item};
use hito_core::test_support::support;
use hito_core::{
    Actor, Credentials, Error, FeaturedShelf, InstallPlugin, OfferPluginService,
    PluginInstallation, Services, UpdateAutomaticLookups,
};
use hito_plugin_sdk::FeaturedKind;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

async fn connect(hito: &Services, actor: &Actor, server: &FeaturedServer) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: format!("Service {}", uuid::Uuid::new_v4()),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}

fn titles(shelf: &FeaturedShelf) -> Vec<String> {
    match shelf {
        FeaturedShelf::Available { items } => {
            items.iter().map(|entry| entry.title.clone()).collect()
        }
        other => panic!(
            "expected available, got {}",
            serde_json::to_string(other).unwrap()
        ),
    }
}

fn status(shelf: &FeaturedShelf) -> Value {
    serde_json::to_value(shelf).unwrap()["status"].clone()
}

#[tokio::test]
async fn featured_items_come_from_an_approved_provider_without_sending_account_data() {
    let mut server = FeaturedServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-read").await;
    let installation = connect(&hito, &actor, &server).await;
    let mut second = item("b");
    second.kind = FeaturedKind::Album;
    second.subtitle = None;
    second.artwork_id = None;
    second.item_count = None;
    server.reply(Reply::Items(vec![item("a"), second]));

    let shelf = hito.catalog.featured(&actor).await.unwrap();
    let wire = serde_json::to_value(&shelf).unwrap();
    assert_eq!(wire["status"], "available");
    assert_eq!(
        wire["items"][0],
        json!({
            "installation_id": installation.id,
            "installation_revision": installation.revision,
            "provider": "Owned featured service",
            "id": "a",
            "title": "Featured a",
            "subtitle": "By a",
            "kind": "playlist",
            "reference": "https://provider.example/playlist/a",
            "artwork_id": "art-a",
            "item_count": 12
        })
    );
    assert_eq!(wire["items"][1]["kind"], "album");
    assert_eq!(wire["items"][1]["subtitle"], Value::Null);

    let requests = server.featured_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].body, json!({"limit": 12}));
    assert!(requests[0].headers.get("cookie").is_none());
    assert!(requests[0].headers.get("authorization").is_none());
    assert_eq!(requests[0].headers["x-hito-plugin-protocol"], "1");
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_second_read_is_served_from_a_short_cache_per_connection() {
    let server = FeaturedServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-cache").await;
    connect(&hito, &actor, &server).await;
    server.reply(Reply::Items(vec![item("a")]));

    for _ in 0..3 {
        assert_eq!(
            titles(&hito.catalog.featured(&actor).await.unwrap()),
            ["Featured a"]
        );
    }
    assert_eq!(server.featured_requests().len(), 1);
}

#[tokio::test]
async fn a_failing_provider_hides_the_shelf_and_is_not_asked_again_at_once() {
    let server = FeaturedServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-failure").await;
    connect(&hito, &actor, &server).await;
    server.reply(Reply::Status(StatusCode::BAD_GATEWAY));

    for _ in 0..2 {
        let shelf = hito.catalog.featured(&actor).await.unwrap();
        assert_eq!(status(&shelf), "unavailable");
    }
    assert_eq!(server.featured_requests().len(), 1);
}

#[tokio::test]
async fn an_empty_answer_is_unavailable_not_an_empty_shelf() {
    let server = FeaturedServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-empty").await;
    connect(&hito, &actor, &server).await;
    server.reply(Reply::Items(Vec::new()));
    assert_eq!(
        status(&hito.catalog.featured(&actor).await.unwrap()),
        "unavailable"
    );
}

async fn rejected(raw: Value) {
    let server = FeaturedServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-invalid").await;
    connect(&hito, &actor, &server).await;
    server.reply(Reply::Raw(raw));
    assert_eq!(
        status(&hito.catalog.featured(&actor).await.unwrap()),
        "unavailable"
    );
    assert_eq!(server.featured_requests().len(), 1);
}

#[tokio::test]
async fn a_page_that_repeats_an_identity_is_rejected_whole() {
    rejected(json!({"items": [item_json("a"), item_json("a")]})).await;
}

#[tokio::test]
async fn a_page_with_an_unknown_field_is_rejected_whole() {
    rejected(json!({"items": [item_json("a")], "next": "x"})).await;
}

#[tokio::test]
async fn a_page_over_the_item_limit_is_rejected_whole() {
    rejected(json!({"items": (0..25).map(|n| item_json(&n.to_string())).collect::<Vec<_>>()}))
        .await;
}

#[tokio::test]
async fn a_page_with_control_characters_is_rejected_whole() {
    rejected(
        json!({"items": [{"id": "a", "title": "bad\nline", "subtitle": null,
        "kind": "playlist", "reference": "r", "artwork_id": null, "item_count": null}]}),
    )
    .await;
}

fn item_json(id: &str) -> Value {
    serde_json::to_value(item(id)).unwrap()
}

#[tokio::test]
async fn accounts_without_a_featured_provider_contact_no_service() {
    let plain = FeaturedServer::without_featured().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-unsupported").await;
    assert_eq!(
        status(&hito.catalog.featured(&actor).await.unwrap()),
        "unsupported"
    );
    connect(&hito, &actor, &plain).await;
    assert_eq!(
        status(&hito.catalog.featured(&actor).await.unwrap()),
        "unsupported"
    );
    assert!(plain.featured_requests().is_empty());
}

#[tokio::test]
async fn turning_off_automatic_service_search_stops_the_shelf_before_any_provider_call() {
    let server = FeaturedServer::start().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-consent").await;
    connect(&hito, &actor, &server).await;
    server.reply(Reply::Items(vec![item("a")]));
    let choice = |on: bool| UpdateAutomaticLookups {
        automatic_artist_lookup: None,
        automatic_artwork: None,
        automatic_service_search: Some(on),
    };
    hito.artist_catalog
        .set_automatic_lookups(&actor, choice(false))
        .await
        .unwrap();
    assert_eq!(
        status(&hito.catalog.featured(&actor).await.unwrap()),
        "disabled"
    );
    assert!(server.featured_requests().is_empty());
    hito.artist_catalog
        .set_automatic_lookups(&actor, choice(true))
        .await
        .unwrap();
    assert_eq!(
        titles(&hito.catalog.featured(&actor).await.unwrap()),
        ["Featured a"]
    );
}

#[tokio::test]
async fn a_slow_provider_is_cut_off_and_a_fast_one_still_answers() {
    let slow = FeaturedServer::start().await;
    let fast = FeaturedServer::with_capabilities(
        "org.hito.fast-fixture",
        "Fast service",
        [
            hito_plugin_sdk::Capability::Playlists,
            hito_plugin_sdk::Capability::Featured,
        ],
    )
    .await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-slow").await;
    connect(&hito, &actor, &slow).await;
    connect(&hito, &actor, &fast).await;
    slow.reply(Reply::Items(vec![item("slow")]));
    slow.delay_featured(Duration::from_secs(30));
    fast.reply(Reply::Items(vec![item("fast")]));

    let started = Instant::now();
    let shelf = hito.catalog.featured(&actor).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(8));
    assert_eq!(titles(&shelf), ["Featured fast"]);
}

#[tokio::test]
async fn several_providers_share_the_shelf_in_turn_and_removal_ends_the_reads() {
    let first = FeaturedServer::start().await;
    let second = FeaturedServer::with_capabilities(
        "org.hito.second-fixture",
        "Second service",
        [
            hito_plugin_sdk::Capability::Playlists,
            hito_plugin_sdk::Capability::Featured,
        ],
    )
    .await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-several").await;
    let one = connect(&hito, &actor, &first).await;
    connect(&hito, &actor, &second).await;
    first.reply(Reply::Items(vec![item("a1"), item("a2")]));
    second.reply(Reply::Items(vec![item("b1"), item("b2")]));

    let shelf = hito.catalog.featured(&actor).await.unwrap();
    let mut seen = titles(&shelf);
    seen.sort();
    assert_eq!(
        seen,
        ["Featured a1", "Featured a2", "Featured b1", "Featured b2"]
    );
    let order = titles(&shelf);
    assert_ne!(order[0][..10], order[1][..10]);

    hito.plugins
        .remove(&actor, &one.id, one.revision)
        .await
        .unwrap();
    assert_eq!(
        titles(&hito.catalog.featured(&actor).await.unwrap()),
        ["Featured b1", "Featured b2"]
    );
}

#[tokio::test]
async fn another_account_never_sees_or_reuses_a_connection_it_does_not_have() {
    let server = FeaturedServer::start().await;
    let (_dir, hito, admin, _library, _music) = support::library("featured-accounts").await;
    connect(&hito, &admin, &server).await;
    server.reply(Reply::Items(vec![item("a")]));
    hito.identity
        .create_user(
            &admin,
            Credentials {
                username: "featured-guest".into(),
                password: "owned guest featured password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "featured-guest".into(),
            password: "owned guest featured password".into(),
        })
        .await
        .unwrap();
    let guest = hito.identity.authenticate(&session.token).await.unwrap();
    assert_eq!(
        titles(&hito.catalog.featured(&admin).await.unwrap()),
        ["Featured a"]
    );
    assert_eq!(
        status(&hito.catalog.featured(&guest).await.unwrap()),
        "unsupported"
    );
    assert_eq!(server.featured_requests().len(), 1);
}

#[tokio::test]
async fn featured_artwork_is_fetched_through_the_approved_connection_only() {
    let server = FeaturedServer::start().await;
    let plain = FeaturedServer::without_featured().await;
    let (_dir, hito, actor, _library, _music) = support::library("featured-artwork").await;
    let installation = connect(&hito, &actor, &server).await;
    let unapproved = connect(&hito, &actor, &plain).await;

    let cover = hito
        .catalog
        .featured_artwork(&actor, &installation.id, installation.revision, "art-a")
        .await
        .unwrap();
    assert_eq!(cover.content_type, "image/png");
    assert!(cover.bytes.starts_with(b"\x89PNG"));
    assert_eq!(
        server.artwork_requests()[0].body,
        json!({"entity": "featured", "provider_id": "art-a"})
    );

    assert!(matches!(
        hito.catalog
            .featured_artwork(&actor, &installation.id, installation.revision + 1, "art-a")
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        hito.catalog
            .featured_artwork(&actor, &unapproved.id, unapproved.revision, "art-a")
            .await,
        Err(Error::Invalid(_))
    ));
    for bad in ["", "   ", "line\nbreak", &"a".repeat(257)] {
        assert!(matches!(
            hito.catalog
                .featured_artwork(&actor, &installation.id, installation.revision, bad)
                .await,
            Err(Error::Invalid(_))
        ));
    }
    server.cover(Cover::Missing);
    assert!(matches!(
        hito.catalog
            .featured_artwork(&actor, &installation.id, installation.revision, "art-a")
            .await,
        Err(Error::NotFound)
    ));
    server.cover(Cover::Bytes(
        b"<html>not an image</html>".to_vec(),
        "image/png",
    ));
    assert!(matches!(
        hito.catalog
            .featured_artwork(&actor, &installation.id, installation.revision, "art-a")
            .await,
        Err(Error::Upstream(_))
    ));
    assert!(plain.artwork_requests().is_empty());
}
