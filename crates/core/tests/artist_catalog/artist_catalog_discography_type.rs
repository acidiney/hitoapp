#[path = "../fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::artist_catalog_discography_service as provider;
use hito_core::test_support::support;

use hito_core::{ArtistDiscographyAvailability, Error, ReadArtistDiscography};
use hito_plugin_sdk::{CatalogReleaseGroupKind, CatalogReleaseTypeFilter};
use serde_json::{Value, json};

const FILTER_CAPABILITIES: &str = "/prefix/v1/catalog/discography/capabilities";
const DISCOGRAPHY: &str = "/prefix/v1/catalog/artists/discography";

async fn read(
    library: &library::Library,
    association: &str,
    release_type: Option<CatalogReleaseTypeFilter>,
    continuation: Option<String>,
) -> hito_core::Result<ArtistDiscographyAvailability> {
    library
        .hito
        .artist_catalog
        .recording_artist_discography_until(
            &library.actor,
            &library.recording_id,
            association,
            ReadArtistDiscography {
                limit: 1,
                continuation,
                release_type,
            },
            std::future::pending(),
        )
        .await
}

fn pages(server: &provider::CatalogServer) -> Vec<Value> {
    server
        .requests()
        .into_iter()
        .filter(|request| request.path == DISCOGRAPHY)
        .map(|request| request.body)
        .collect()
}

#[tokio::test]
async fn a_typed_read_negotiates_the_filter_pages_one_type_and_fences_its_continuation() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    server.enable_discography();
    server.enable_type_filter();

    let ArtistDiscographyAvailability::Available {
        page, continuation, ..
    } = read(
        &library,
        &association.id,
        Some(CatalogReleaseTypeFilter::Album),
        None,
    )
    .await
    .unwrap()
    else {
        panic!("a typed first page must be available");
    };
    assert_eq!(page.items.len(), 1);
    assert!(page.items[0].kind == Some(CatalogReleaseGroupKind::Album));
    let continuation = continuation.expect("the second album page needs a host continuation");

    let before = server.requests();
    for (filter, token) in [
        (Some(CatalogReleaseTypeFilter::Ep), continuation.clone()),
        (None, continuation.clone()),
    ] {
        assert!(
            matches!(
                read(&library, &association.id, filter, Some(token)).await,
                Err(Error::Invalid(_))
            ),
            "a typed continuation must not continue another type or the whole catalog"
        );
    }
    assert_eq!(
        server.requests(),
        before,
        "refusals make no provider request"
    );

    let ArtistDiscographyAvailability::Available {
        page,
        continuation: last,
        ..
    } = read(
        &library,
        &association.id,
        Some(CatalogReleaseTypeFilter::Album),
        Some(continuation),
    )
    .await
    .unwrap()
    else {
        panic!("the typed continuation must be available");
    };
    assert_eq!(page.items[0].id, "opaque:release-group/album-2");
    assert!(last.is_none());

    let ArtistDiscographyAvailability::Available { page, .. } = read(
        &library,
        &association.id,
        Some(CatalogReleaseTypeFilter::Ep),
        None,
    )
    .await
    .unwrap() else {
        panic!("the EP page must be available");
    };
    assert!(page.items[0].kind == Some(CatalogReleaseGroupKind::Ep));

    assert_eq!(
        pages(&server),
        vec![
            json!({"artist_id": provider::ARTIST_ID, "cursor": null, "snapshot": null, "limit": 1, "release_type": "album"}),
            json!({"artist_id": provider::ARTIST_ID, "cursor": provider::TYPED_CURSOR, "snapshot": null, "limit": 1, "release_type": "album"}),
            json!({"artist_id": provider::ARTIST_ID, "cursor": null, "snapshot": null, "limit": 1, "release_type": "ep"}),
        ],
        "only the provider cursor and the negotiated type reach the provider"
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request.path == FILTER_CAPABILITIES),
        "the filter is negotiated before a typed page is requested"
    );
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_whole_catalog_read_never_negotiates_or_sends_the_filter() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    server.enable_discography();
    server.enable_type_filter();
    assert!(matches!(
        read(&library, &association.id, None, None).await.unwrap(),
        ArtistDiscographyAvailability::Available { .. }
    ));
    assert_eq!(
        pages(&server),
        vec![
            json!({"artist_id": provider::ARTIST_ID, "cursor": null, "snapshot": null, "limit": 1})
        ],
        "an unfiltered request keeps its earlier exact body"
    );
    assert!(
        !server
            .requests()
            .iter()
            .any(|request| request.path == FILTER_CAPABILITIES)
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_provider_without_the_filter_reports_it_and_still_serves_the_whole_catalog() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    server.enable_discography();

    let outcome = read(
        &library,
        &association.id,
        Some(CatalogReleaseTypeFilter::Album),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        outcome,
        ArtistDiscographyAvailability::TypeFilterUnsupported
    ));
    assert!(pages(&server).is_empty(), "no typed page may be requested");
    assert!(matches!(
        read(&library, &association.id, None, None).await.unwrap(),
        ArtistDiscographyAvailability::Available { .. }
    ));
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_typed_provider_outage_is_unavailable_and_leaves_the_whole_catalog_readable() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    server.enable_discography();
    server.enable_type_filter();
    server.fail_typed_reads(true);
    let outcome = read(
        &library,
        &association.id,
        Some(CatalogReleaseTypeFilter::Album),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        outcome,
        ArtistDiscographyAvailability::Unavailable
    ));
    assert!(matches!(
        read(&library, &association.id, None, None).await.unwrap(),
        ArtistDiscographyAvailability::Available { .. }
    ));
    server.fail_typed_reads(false);
    assert!(matches!(
        read(
            &library,
            &association.id,
            Some(CatalogReleaseTypeFilter::Album),
            None
        )
        .await
        .unwrap(),
        ArtistDiscographyAvailability::Available { .. }
    ));
    assert!(server.finish().await);
}
