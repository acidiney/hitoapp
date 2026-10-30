#[path = "../fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::artist_catalog_editions_service as provider;
use hito_core::test_support::support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    ArtistDiscographyAvailability, EditionTrackListAvailability, Error, ReadArtistDiscography,
    ReadEditionTrackList, ReadReleaseGroupEditions, ReleaseGroupEditionsAvailability,
    UpdateMetadataSettings,
};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::time::timeout;

#[tokio::test]
async fn a_reviewed_artist_group_lists_editions_and_reads_only_the_explicit_choice() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(timeout(
        Duration::from_secs(30),
        exercise(&library, &server),
    ))
    .catch_unwind()
    .await;
    let joined = server.finish().await;
    let result = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert!(joined, "The owned edition HTTP server must join shutdown");
    result.expect("The bounded owned edition workflow must complete");
}

async fn exercise(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    let evidence = serde_json::to_value(&association).unwrap();
    let before = library.snapshot().await;
    let events_before =
        serde_json::to_value(hito.collection.events(actor, 0).await.unwrap()).unwrap();
    assert_no_downloads(library).await;

    let groups = hito
        .artist_catalog
        .recording_artist_discography_until(
            actor,
            &library.recording_id,
            &association.id,
            ReadArtistDiscography {
                limit: 1,
                continuation: None,
                release_type: None,
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistDiscographyAvailability::Available {
        association_id,
        page,
        ..
    } = groups
    else {
        panic!("The chosen provider artist must supply the release-group page");
    };
    assert_eq!(association_id, association.id);
    let group = page
        .items
        .iter()
        .find(|group| group.id == provider::GROUP_ID)
        .expect("The user-selected group must come from the provider artist's page");
    assert_eq!(group.title, "Owned album");
    assert!(
        server
            .requests()
            .iter()
            .all(|request| !request.path.contains("/editions"))
    );

    let before_unsupported = server.requests();
    let unsupported = read_editions(library, &association.id, &group.id, None)
        .await
        .unwrap();
    assert!(
        matches!(unsupported, ReleaseGroupEditionsAvailability::Unsupported),
        "Track-list support and legacy catalog operations must not authorize edition listing"
    );
    let unsupported_requests = server.requests();
    assert_eq!(unsupported_requests.len(), before_unsupported.len() + 1);
    assert!(unsupported_requests.last().is_some_and(|request| {
        request.method == Method::GET
            && request.path == "/prefix/v1/catalog/editions/capabilities"
            && request.body.is_null()
    }));

    server.enable_edition_listing();
    let first = read_editions(library, &association.id, &group.id, None)
        .await
        .unwrap();
    let ReleaseGroupEditionsAvailability::Available {
        association_id,
        page: first_page,
        continuation: Some(continuation),
    } = first
    else {
        panic!("The declared edition-list operation must return a bounded first page");
    };
    assert_eq!(association_id, association.id);
    assert!(first_page == provider::expected_editions(false));
    assert!(continuation.is_ascii() && !continuation.is_empty() && continuation.len() <= 8192);
    assert_ne!(continuation, provider::PROVIDER_CURSOR);
    let last = read_editions(
        library,
        &association.id,
        &group.id,
        Some(continuation.clone()),
    )
    .await
    .unwrap();
    let ReleaseGroupEditionsAvailability::Available {
        association_id,
        page: last_page,
        continuation: last_continuation,
    } = last
    else {
        panic!("The same authority must permit the next edition page");
    };
    assert_eq!(association_id, association.id);
    assert!(last_page == provider::expected_editions(true));
    assert!(last_continuation.is_none());
    assert_eq!(first_page.items[0].title, last_page.items[0].title);
    assert_ne!(first_page.items[0].id, last_page.items[0].id);
    assert!(
        server
            .requests()
            .iter()
            .all(|request| { request.path != "/prefix/v1/catalog/editions/track-list" }),
        "Reading edition summaries must not eagerly fetch track lists"
    );

    let selected = last_page
        .items
        .iter()
        .find(|edition| edition.id == provider::BONUS_ID)
        .expect("The bonus edition must be explicitly selected by its returned identity");
    let result = hito
        .artist_catalog
        .recording_artist_edition_track_list_until(
            actor,
            &library.recording_id,
            &association.id,
            ReadEditionTrackList {
                release_group_id: group.id.clone(),
                edition_id: selected.id.clone(),
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    let EditionTrackListAvailability::Available {
        association_id,
        edition,
    } = result
    else {
        panic!("The exact selected edition must return metadata for every occurrence");
    };
    assert_eq!(association_id, association.id);
    assert!(edition == provider::expected_track_list());
    assert_ne!(edition.media[0].tracks[0].id, edition.media[1].tracks[0].id);
    assert_eq!(
        edition.media[0].tracks[0].recording_id,
        edition.media[1].tracks[0].recording_id
    );

    let requests = server.requests();
    let pages = requests
        .iter()
        .filter(|request| request.path == "/prefix/v1/catalog/release-groups/editions")
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 2);
    for (request, cursor) in pages
        .iter()
        .zip([Value::Null, json!(provider::PROVIDER_CURSOR)])
    {
        assert!(request.method == Method::POST);
        assert_eq!(
            request.body,
            json!({
                "release_group_id": group.id, "cursor": cursor, "snapshot": null, "limit": 1
            })
        );
    }
    let tracks = requests
        .iter()
        .filter(|request| request.path == "/prefix/v1/catalog/editions/track-list")
        .collect::<Vec<_>>();
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].method == Method::POST);
    assert_eq!(
        tracks[0].body,
        json!({"release_group_id": group.id, "edition_id": selected.id})
    );

    let settings = hito.artist_catalog.metadata_settings(actor).await.unwrap();
    let revised = hito
        .artist_catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: settings.providers,
            },
        )
        .await
        .unwrap();
    assert!(revised.revision > settings.revision);
    let before_stale = server.requests();
    let stale = read_editions(library, &association.id, &group.id, Some(continuation)).await;
    assert!(
        matches!(stale, Err(Error::Conflict(_))),
        "A retained edition continuation must conflict after a metadata policy revision"
    );
    assert_eq!(
        server.requests(),
        before_stale,
        "Stale authority must fail before provider I/O"
    );

    assert!(library.snapshot().await == before);
    assert_eq!(
        serde_json::to_value(hito.collection.events(actor, 0).await.unwrap()).unwrap(),
        events_before
    );
    assert_no_downloads(library).await;
    let retained = hito
        .artist_catalog
        .recording_artist_associations(actor, &library.recording_id)
        .await
        .unwrap();
    assert!(retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == evidence);
    assert!(
        hito.artist_catalog
            .recording_artist_associations(actor, &library.other_recording_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.unexpected(), 0);
    assert!(
        server.requests().iter().all(|request| matches!(
            request.path.as_str(),
            "/prefix/v1/manifest"
                | "/prefix/v1/catalog/capabilities"
                | "/prefix/v1/catalog/artists/profile"
                | "/prefix/v1/catalog/artists/discography"
                | "/prefix/v1/catalog/editions/capabilities"
                | "/prefix/v1/catalog/release-groups/editions"
                | "/prefix/v1/catalog/editions/track-list"
        )),
        "Edition metadata must not request sources, audio, downloads or adoption"
    );
}

async fn read_editions(
    library: &library::Library,
    association: &str,
    group: &str,
    continuation: Option<String>,
) -> hito_core::Result<ReleaseGroupEditionsAvailability> {
    library
        .hito
        .artist_catalog
        .recording_artist_release_group_editions_until(
            &library.actor,
            &library.recording_id,
            association,
            ReadReleaseGroupEditions {
                release_group_id: group.into(),
                limit: 1,
                continuation,
            },
            std::future::pending(),
        )
        .await
}

async fn assert_no_downloads(library: &library::Library) {
    for recording in [&library.recording_id, &library.other_recording_id] {
        assert!(
            matches!(
                library
                    .hito
                    .downloads
                    .status(&library.actor, recording)
                    .await,
                Err(Error::NotFound)
            ),
            "Catalog edition browsing must preserve explicit-download absence"
        );
    }
}
