#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    ArtistCandidateReleasesAvailability, ArtistCandidateSearchAvailability, ArtistReviewContext,
    MetadataProvider, UpdateMetadataSettings,
};
use hito_plugin_sdk::ArtistSearchRequest;
use library::{CREDIT, NAMESAKE, NOVA, NameLibrary};

async fn searched_context(library: &NameLibrary) -> ArtistReviewContext {
    library.musicbrainz.search(
        CREDIT,
        Reply::Candidates(vec![
            (NOVA.into(), CREDIT.into()),
            (NAMESAKE.into(), CREDIT.into()),
        ]),
    );
    let context = library
        .hito
        .artist_catalog
        .begin_artist_review(
            &library.actor,
            &library.recordings[0],
            MetadataProvider::BuiltinMusicBrainz,
        )
        .await
        .unwrap();
    let found = library
        .hito
        .artist_catalog
        .search_artist_candidates_until(
            &library.actor,
            &context,
            ArtistSearchRequest {
                query: CREDIT.into(),
                cursor: None,
                snapshot: None,
                limit: 20,
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    let ArtistCandidateSearchAvailability::Available { page, context } = found else {
        panic!("the built-in search must answer");
    };
    assert_eq!(page.items.len(), 2);
    *context
}

async fn releases(
    library: &NameLibrary,
    context: &ArtistReviewContext,
    artist: &str,
) -> ArtistCandidateReleasesAvailability {
    library
        .hito
        .artist_catalog
        .known_releases_of_artist_candidate_until(
            &library.actor,
            context,
            artist,
            std::future::pending(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_candidate_lists_at_most_three_release_titles_with_one_bounded_request_asked_only_for_that_candidate()
 {
    let library = NameLibrary::nova().await;
    let context = searched_context(&library).await;
    library.musicbrainz.release_groups_of(
        NOVA,
        None,
        Reply::ReleaseGroupTitles(
            [
                "Morning Light",
                "Second Wind",
                "Harbor Lights",
                "Quiet Rooms",
                "Late Tide",
            ]
            .map(String::from)
            .to_vec(),
        ),
    );
    library.musicbrainz.release_groups_of(
        NAMESAKE,
        None,
        Reply::ReleaseGroupTitles(vec!["Only Record".into()]),
    );
    let before = library.musicbrainz.requests().len();

    let ArtistCandidateReleasesAvailability::Available { titles, .. } =
        releases(&library, &context, NOVA).await
    else {
        panic!("the built-in release list must be available");
    };
    assert_eq!(titles, ["Morning Light", "Second Wind", "Harbor Lights"]);
    let requests = library.musicbrainz.requests();
    assert_eq!(requests.len(), before + 1);
    assert!(requests[before].contains("limit=3") && !requests[before].contains("type="));

    let ArtistCandidateReleasesAvailability::Available { titles, .. } =
        releases(&library, &context, NAMESAKE).await
    else {
        panic!("the namesake release list must be available");
    };
    assert_eq!(titles, ["Only Record"]);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn release_titles_report_unavailable_providers_reject_foreign_ids_and_respect_disabled_metadata()
 {
    let library = NameLibrary::nova().await;
    let context = searched_context(&library).await;
    library
        .musicbrainz
        .release_groups_of(NOVA, None, Reply::Unavailable);
    assert!(matches!(
        releases(&library, &context, NOVA).await,
        ArtistCandidateReleasesAvailability::Unavailable
    ));
    assert!(
        library
            .hito
            .artist_catalog
            .known_releases_of_artist_candidate_until(
                &library.actor,
                &context,
                "not-an-id",
                std::future::pending(),
            )
            .await
            .is_err()
    );

    let settings = library
        .hito
        .artist_catalog
        .metadata_settings(&library.actor)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .set_metadata_settings(
            &library.actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: settings.providers,
            },
        )
        .await
        .unwrap();
    let before = library.musicbrainz.requests().len();
    assert!(matches!(
        releases(&library, &context, NOVA).await,
        ArtistCandidateReleasesAvailability::Disabled
    ));
    assert_eq!(library.musicbrainz.requests().len(), before);
    library.origin.shutdown().await;
}
