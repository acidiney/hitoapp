use hito_core::test_support::collection_artwork as fixture;
use hito_core::test_support::support;
use hito_core::{
    Actor, ImportProviderPlaylist, PluginInstallation, Services, UpdateAutomaticLookups,
};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use uuid::Uuid;

struct Imported {
    hito: Services,
    actor: Actor,
    provider: fixture::ArtworkServer,
    release: String,
    _directory: tempfile::TempDir,
}

async fn imported() -> Imported {
    let provider = fixture::ArtworkServer::start().await;
    let (directory, hito, actor, _library, _music) = support::library("provider-album-art").await;
    let installed: PluginInstallation = provider.connect(&hito, &actor).await;
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
    let albums = hito
        .catalog
        .releases(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(albums.total, 1);
    Imported {
        release: albums.items[0].id.clone(),
        hito,
        actor,
        provider,
        _directory: directory,
    }
}

async fn work(hito: &Services) -> bool {
    hito.catalog
        .run_next_artwork_job_until(std::future::pending::<()>())
        .await
        .unwrap()
}

async fn automatic_artwork(hito: &Services, actor: &Actor, on: bool) {
    hito.artist_catalog
        .set_automatic_lookups(
            actor,
            UpdateAutomaticLookups {
                automatic_artist_lookup: None,
                automatic_artwork: Some(on),
                automatic_service_search: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn reading_an_imported_album_cover_queues_the_provider_cover_once() {
    let t = imported().await;
    for _ in 0..2 {
        assert!(
            t.hito
                .catalog
                .release_artwork(&t.actor, &t.release)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert!(work(&t.hito).await);
    assert!(!work(&t.hito).await);
    assert_eq!(
        t.provider.requests().await,
        [ArtworkRequest {
            entity: ArtworkEntity::Release,
            provider_id: fixture::RELEASE_ID.into()
        }]
    );
    let cover = t
        .hito
        .catalog
        .release_artwork(&t.actor, &t.release)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cover.bytes, fixture::RELEASE_COVER);
    assert!(!work(&t.hito).await);
    assert_eq!(t.provider.requests().await.len(), 1);
}

#[tokio::test]
async fn turning_automatic_artwork_off_stops_provider_album_covers() {
    let t = imported().await;
    automatic_artwork(&t.hito, &t.actor, false).await;
    assert!(
        t.hito
            .catalog
            .release_artwork(&t.actor, &t.release)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!work(&t.hito).await);
    assert!(
        !t.hito
            .catalog
            .release_artwork_availability(&t.actor, &t.release)
            .await
            .unwrap()
            .can_fetch
    );
    assert!(t.provider.requests().await.is_empty());
    t.hito
        .catalog
        .enqueue_release_artwork(&t.actor, &t.release)
        .await
        .unwrap();
    assert!(work(&t.hito).await);
    automatic_artwork(&t.hito, &t.actor, true).await;
    assert!(
        t.hito
            .catalog
            .release_artwork_availability(&t.actor, &t.release)
            .await
            .unwrap()
            .can_fetch
    );
}
