#[allow(dead_code)]
#[path = "../fixtures/artist_isrc_library.rs"]
mod library;

use hito_core::ArtistAssociationEvidence;
use library::{ARTIST, Library, OTHER_ARTIST, PAGE, SECOND_PAGE, SHARED_PAGE, UNLINKED_PAGE};

async fn only_association(library: &Library, recording: &str) -> hito_core::ArtistAssociation {
    let mut linked = library
        .hito
        .artist_catalog
        .recording_artist_associations(&library.actor, recording)
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    linked.remove(0)
}

#[tokio::test]
async fn a_lone_provider_artist_page_that_musicbrainz_links_to_one_artist_joins_that_artist() {
    let library = Library::prepare().await;
    library.tag_pass().await;
    let tagged = library.group_of(&library.tagged).await.unwrap();
    library.run().await;

    assert_eq!(library.group_of(&library.page).await, Some(tagged));
    let linked = only_association(&library, &library.page).await;
    assert_eq!(linked.provider_artist_id, ARTIST);
    assert_eq!(
        linked.evidence,
        ArtistAssociationEvidence::MatchedProviderUrl
    );

    let by_page = only_association(&library, &library.page_with_isrc).await;
    assert_eq!(by_page.provider_artist_id, OTHER_ARTIST);
    assert_eq!(
        by_page.evidence,
        ArtistAssociationEvidence::MatchedProviderUrl,
        "An ISRC MusicBrainz does not know falls through to the lone artist page"
    );
    let by_isrc = only_association(&library, &library.imported).await;
    assert_eq!(by_isrc.evidence, ArtistAssociationEvidence::MatchedIsrc);

    for uncertain in [
        &library.shared_page,
        &library.unlinked_page,
        &library.two_pages,
    ] {
        assert!(library.group_of(uncertain).await.is_none());
    }
    let lookups = library.musicbrainz.requests();
    let page_lookups: Vec<&String> = lookups
        .iter()
        .filter(|request| request.starts_with("/ws/2/url?"))
        .collect();
    assert_eq!(page_lookups.len(), 5, "{lookups:?}");
    for page in [PAGE, SECOND_PAGE, SHARED_PAGE, UNLINKED_PAGE] {
        let encoded = page.replace(':', "%3A").replace('/', "%2F");
        assert!(
            page_lookups
                .iter()
                .any(|request| request.contains(&format!("resource={encoded}&inc=artist-rels"))),
            "{page}"
        );
    }
    library.origin.shutdown().await;
}
