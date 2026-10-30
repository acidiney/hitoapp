#[path = "../fixtures/artist_catalog_profiles_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::artist_catalog_discography_service as provider;
use hito_core::test_support::support;

use axum::http::Method;
use futures_util::FutureExt;
use hito_core::{
    ArtistCandidatePreviewAvailability, ArtistDiscographyAvailability, Error, InstallPlugin,
    MetadataProvider, OfferPluginService, ReadArtistDiscography, UpdateMetadataSettings,
};
use hito_plugin_sdk::ArtistDiscographyRequest;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::time::timeout;

#[tokio::test]
async fn a_reviewed_recording_discography_pages_without_adoption_and_fences_its_continuation() {
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
    assert!(
        joined,
        "The owned discography HTTP server must join shutdown"
    );
    result.expect("The bounded owned discography workflow must complete");
}

async fn exercise(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let actor = &library.actor;
    let service = hito
        .plugins
        .offer_service(
            &library.admin,
            OfferPluginService {
                name: "Owned artist discography".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
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
        .unwrap();
    let defaults = hito.catalog.metadata_settings(actor).await.unwrap();
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let settings = hito
        .catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: defaults.revision,
                enabled: true,
                providers: vec![selected.clone()],
            },
        )
        .await
        .unwrap();
    let before = library.snapshot().await;
    let events_before =
        serde_json::to_value(hito.collection.events(actor, 0).await.unwrap()).unwrap();
    assert_no_downloads(library).await;
    let context = hito
        .catalog
        .begin_artist_review(actor, &library.recording_id, selected.clone())
        .await
        .unwrap();
    let preview = hito
        .catalog
        .preview_artist_candidate_until(
            actor,
            &context,
            provider::ARTIST_ID,
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistCandidatePreviewAvailability::Available { profile, .. } = preview else {
        panic!("The exact provider artist must be previewed before confirmation");
    };
    assert!(profile == provider::expected_profile());
    let association = hito
        .catalog
        .confirm_artist_review(actor, &context, &profile.artist.id)
        .await
        .unwrap();
    assert!(association.provider == selected);
    assert!(association.provider_artist_id == provider::ARTIST_ID);
    let evidence = serde_json::to_value(&association).unwrap();
    let before_invalid = server.requests();
    for limit in [0, 51] {
        let invalid = hito
            .catalog
            .recording_artist_discography_until(
                actor,
                &library.recording_id,
                &association.id,
                ReadArtistDiscography {
                    limit,
                    continuation: None,
                },
                std::future::pending(),
            )
            .await;
        assert!(
            matches!(invalid, Err(Error::Invalid(_))),
            "Catalog page limits must stay between one and 50 without provider I/O"
        );
    }
    assert_eq!(server.requests(), before_invalid);
    let before_unsupported = server.requests();
    let unsupported = read(library, &association.id, None).await.unwrap();
    assert!(
        matches!(unsupported, ArtistDiscographyAvailability::Unsupported),
        "A profile-only catalog declaration must not authorize discography reads"
    );
    assert_eq!(server.requests().len(), before_unsupported.len() + 1);
    assert!(
        server.requests().last().is_some_and(|request| {
            request.method == Method::GET
                && request.path == "/prefix/v1/catalog/capabilities"
                && request.body.is_null()
        }),
        "Discography must negotiate its declared operation without sending a page request"
    );

    server.enable_discography();
    let first = read(library, &association.id, None).await.unwrap();
    let ArtistDiscographyAvailability::Available {
        association_id,
        page: first_page,
        continuation,
    } = first
    else {
        panic!("The explicitly confirmed association must return its first catalog page");
    };
    assert_eq!(association_id, association.id);
    first_page
        .validate_for(&ArtistDiscographyRequest {
            artist_id: provider::ARTIST_ID.into(),
            cursor: None,
            snapshot: None,
            limit: 1,
        })
        .unwrap();
    assert!(first_page == provider::expected_page(false));
    let continuation = continuation.expect("A provider continuation requires a host continuation");
    assert!(
        continuation.is_ascii()
            && !continuation.is_empty()
            && continuation.len() <= 8192
            && continuation != provider::PROVIDER_CURSOR,
        "The application must return a bounded host token, separate from the provider cursor"
    );
    let mut changed = continuation.clone().into_bytes();
    let middle = changed.len() / 2;
    changed[middle] = if changed[middle] == b'A' { b'B' } else { b'A' };
    let before_tamper = server.requests();
    let tampered = read(
        library,
        &association.id,
        Some(String::from_utf8(changed).unwrap()),
    )
    .await;
    assert!(
        matches!(tampered, Err(Error::Invalid(_))),
        "Changing a middle token byte must fail authentication before provider I/O"
    );
    assert_eq!(server.requests(), before_tamper);

    let last = read(library, &association.id, Some(continuation.clone()))
        .await
        .unwrap();
    let ArtistDiscographyAvailability::Available {
        association_id,
        page: last_page,
        continuation: last_continuation,
    } = last
    else {
        panic!("The unchanged recording authority must allow its next catalog page");
    };
    assert_eq!(association_id, association.id);
    last_page
        .validate_for(&ArtistDiscographyRequest {
            artist_id: provider::ARTIST_ID.into(),
            cursor: Some(provider::PROVIDER_CURSOR.into()),
            snapshot: None,
            limit: 1,
        })
        .unwrap();
    assert!(last_page == provider::expected_page(true) && last_continuation.is_none());
    assert!(
        first_page.items[0].title == last_page.items[0].title
            && first_page.items[0].id != last_page.items[0].id,
        "Same-title live and uncertain catalog groups must retain separate provider identities"
    );
    let pages = server
        .requests()
        .into_iter()
        .filter(|request| request.path == "/prefix/v1/catalog/artists/discography")
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
                "artist_id": provider::ARTIST_ID,
                "cursor": cursor,
                "snapshot": null,
                "limit": 1
            }),
            "Only the association-derived provider identity and provider pagination belong on the wire"
        );
    }

    let revised = hito
        .catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![selected],
            },
        )
        .await
        .unwrap();
    assert!(revised.revision > settings.revision);
    let before_stale = server.requests();
    let stale = read(library, &association.id, Some(continuation)).await;
    assert!(
        matches!(stale, Err(Error::Conflict(_))),
        "An earlier page token must not continue under a new accepted metadata policy revision"
    );
    assert_eq!(server.requests(), before_stale);
    assert!(library.snapshot().await == before);
    assert_no_downloads(library).await;
    assert_eq!(
        serde_json::to_value(hito.collection.events(actor, 0).await.unwrap()).unwrap(),
        events_before,
        "Catalog reads must not append collection events"
    );
    let retained = hito
        .catalog
        .recording_artist_associations(actor, &library.recording_id)
        .await
        .unwrap();
    assert!(retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == evidence);
    assert!(
        hito.catalog
            .recording_artist_associations(actor, &library.other_recording_id)
            .await
            .unwrap()
            .is_empty(),
        "The same credited name on another recording must not inherit this association"
    );
    assert_eq!(server.unexpected(), 0);
    assert!(
        server.requests().iter().all(|request| matches!(
            request.path.as_str(),
            "/prefix/v1/manifest"
                | "/prefix/v1/catalog/capabilities"
                | "/prefix/v1/catalog/artists/profile"
                | "/prefix/v1/catalog/artists/discography"
        )),
        "Catalog browsing must not request sources, audio, downloads or adoption"
    );
}

async fn read(
    library: &library::Library,
    association: &str,
    continuation: Option<String>,
) -> hito_core::Result<ArtistDiscographyAvailability> {
    library
        .hito
        .catalog
        .recording_artist_discography_until(
            &library.actor,
            &library.recording_id,
            association,
            ReadArtistDiscography {
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
            "Artist catalog browsing must preserve explicit-download absence"
        );
    }
}

#[tokio::test]
async fn a_discography_continuation_cannot_move_to_a_new_session_of_the_same_account() {
    let library = library::Library::prepare().await;
    let mut server = provider::CatalogServer::start().await;
    let result = AssertUnwindSafe(timeout(
        Duration::from_secs(30),
        exercise_session_change(&library, &server),
    ))
    .catch_unwind()
    .await;
    let joined = server.finish().await;
    let result = match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    };
    assert!(
        joined,
        "The owned discography HTTP server must join shutdown"
    );
    result.expect("The bounded session-continuity workflow must complete");
}

async fn exercise_session_change(library: &library::Library, server: &provider::CatalogServer) {
    let hito = &library.hito;
    let association = library
        .review_artist(&server.url, provider::ARTIST_ID)
        .await;
    server.enable_discography();
    let session_a = hito
        .identity
        .login(hito_core::Credentials {
            username: "listener".into(),
            password: "artist catalog listener password".into(),
        })
        .await
        .unwrap();
    let actor_a = hito.identity.authenticate(&session_a.token).await.unwrap();
    assert_eq!(actor_a.user().id, library.actor.user().id);
    let before = library.snapshot().await;
    let events_before =
        serde_json::to_value(hito.collection.events(&library.actor, 0).await.unwrap()).unwrap();
    let evidence = serde_json::to_value(&association).unwrap();
    assert_no_downloads(library).await;
    let first = hito
        .catalog
        .recording_artist_discography_until(
            &actor_a,
            &library.recording_id,
            &association.id,
            ReadArtistDiscography {
                limit: 1,
                continuation: None,
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistDiscographyAvailability::Available {
        association_id,
        page,
        continuation: Some(continuation),
    } = first
    else {
        panic!("The originating session must receive its first catalog page and continuation");
    };
    assert_eq!(association_id, association.id);
    assert!(page == provider::expected_page(false));

    hito.identity.logout(&session_a.token).await.unwrap();
    assert!(matches!(
        hito.identity.check_session(&actor_a).await,
        Err(Error::Unauthorized)
    ));
    let session_b = hito
        .identity
        .login(hito_core::Credentials {
            username: "listener".into(),
            password: "artist catalog listener password".into(),
        })
        .await
        .unwrap();
    let actor_b = hito.identity.authenticate(&session_b.token).await.unwrap();
    assert_eq!(actor_b.user().id, actor_a.user().id);
    assert!(!actor_b.same_session(&actor_a));
    hito.identity.check_session(&actor_b).await.unwrap();
    let before_new_session_read = server.requests();
    let continued = hito
        .catalog
        .recording_artist_discography_until(
            &actor_b,
            &library.recording_id,
            &association.id,
            ReadArtistDiscography {
                limit: 1,
                continuation: Some(continuation),
            },
            std::future::pending(),
        )
        .await;
    assert!(
        matches!(continued, Err(Error::Conflict(_))),
        "A fresh authenticated session must start pagination again instead of reusing the prior session's token"
    );
    assert_eq!(server.requests(), before_new_session_read);
    assert!(library.snapshot().await == before);
    assert_no_downloads(library).await;
    assert_eq!(
        serde_json::to_value(hito.collection.events(&actor_b, 0).await.unwrap()).unwrap(),
        events_before,
        "Rejecting a prior-session continuation must not append collection events"
    );
    let retained = hito
        .catalog
        .recording_artist_associations(&actor_b, &library.recording_id)
        .await
        .unwrap();
    assert!(retained.len() == 1 && serde_json::to_value(&retained[0]).unwrap() == evidence);
    assert_eq!(server.unexpected(), 0);
}
