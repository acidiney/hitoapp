#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{ArtistDiscographyAvailability, ReadArtistDiscography};
use hito_plugin_sdk::{CatalogReleaseGroupKind, CatalogReleaseTypeFilter};
use library::{CREDIT, NOVA, NameLibrary, TRIBUTE, nova_discography};

const ALBUM: &str = "0f750000-0000-4000-8000-000000000001";
const SECOND_ALBUM: &str = "0f750000-0000-4000-8000-000000000002";
const SINGLE: &str = "0f750000-0000-4000-8000-000000000003";

async fn read(
    library: &NameLibrary,
    recording: &str,
    association: &str,
    release_type: Option<CatalogReleaseTypeFilter>,
) -> ArtistDiscographyAvailability {
    library
        .hito
        .artist_catalog
        .recording_artist_discography_until(
            &library.actor,
            recording,
            association,
            ReadArtistDiscography {
                limit: 50,
                continuation: None,
                release_type,
            },
            std::future::pending(),
        )
        .await
        .unwrap()
}

fn kinds(availability: &ArtistDiscographyAvailability) -> Vec<Option<CatalogReleaseGroupKind>> {
    let ArtistDiscographyAvailability::Available { page, .. } = availability else {
        panic!("the built-in discography must be available");
    };
    page.items.iter().map(|group| group.kind).collect()
}

#[tokio::test]
async fn the_built_in_provider_pages_one_release_type_with_a_bounded_browse() {
    let library = NameLibrary::nova().await;
    library.musicbrainz.search(
        CREDIT,
        Reply::Candidates(vec![
            (NOVA.into(), CREDIT.into()),
            (TRIBUTE.into(), "Nova Quartet Tribute".into()),
        ]),
    );
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("morning light", Some(202_900)),
            ("Second  Wind", Some(185_500)),
            ("HARBOR LIGHTS", Some(243_000)),
        ]),
    );
    library.run().await;
    let recording = library.recordings[0].clone();
    let association = library.associations(&recording).await.remove(0);
    assert_eq!(association.provider_artist_id, NOVA);

    library.musicbrainz.release_groups_of(
        NOVA,
        Some("album"),
        Reply::ReleaseGroups(vec![
            (ALBUM.into(), Some("Album".into())),
            (SECOND_ALBUM.into(), Some("Album".into())),
        ]),
    );
    library.musicbrainz.release_groups_of(
        NOVA,
        None,
        Reply::ReleaseGroups(vec![
            (ALBUM.into(), Some("Album".into())),
            (SINGLE.into(), Some("Single".into())),
        ]),
    );
    let before = library.musicbrainz.requests().len();

    let typed = read(
        &library,
        &recording,
        &association.id,
        Some(CatalogReleaseTypeFilter::Album),
    )
    .await;
    assert!(kinds(&typed) == vec![Some(CatalogReleaseGroupKind::Album); 2]);
    let whole = read(&library, &recording, &association.id, None).await;
    assert!(
        kinds(&whole)
            == vec![
                Some(CatalogReleaseGroupKind::Album),
                Some(CatalogReleaseGroupKind::Single)
            ]
    );

    let requests = library.musicbrainz.requests();
    assert_eq!(requests.len(), before + 2, "one bounded browse per read");
    assert!(requests[before].contains("type=album") && requests[before].contains("limit=50"));
    assert!(!requests[before + 1].contains("type="));

    library
        .musicbrainz
        .release_groups_of(NOVA, Some("ep"), Reply::Unavailable);
    assert!(matches!(
        read(
            &library,
            &recording,
            &association.id,
            Some(CatalogReleaseTypeFilter::Ep)
        )
        .await,
        ArtistDiscographyAvailability::Unavailable
    ));
    library.origin.shutdown().await;
}
