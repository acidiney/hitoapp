#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::ArtistIdentityGroupQuery;
use hito_core::test_support::musicbrainz::Reply;
use library::{CREDIT, NOVA, NameLibrary, TRIBUTE, nova_discography};

const LONGER: &str = "A Nova Quartet";

#[tokio::test]
async fn an_exact_name_match_leads_the_group_search_whatever_the_alphabet() {
    let mut entries: Vec<(&str, &str, u64)> = vec![
        (CREDIT, "Morning Light", 201_000),
        (CREDIT, "Second Wind", 187_500),
        (LONGER, "Tribute One", 150_000),
        (LONGER, "Tribute Two", 160_000),
    ];
    entries.sort();
    let library = NameLibrary::prepare(&entries).await;
    library.musicbrainz.search(
        CREDIT,
        Reply::Candidates(vec![(NOVA.into(), CREDIT.into())]),
    );
    library.musicbrainz.search(
        LONGER,
        Reply::Candidates(vec![(TRIBUTE.into(), LONGER.into())]),
    );
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
        ]),
    );
    library.musicbrainz.recordings_of(
        TRIBUTE,
        nova_discography(&[
            ("Tribute One", Some(150_000)),
            ("Tribute Two", Some(160_000)),
        ]),
    );
    library.automatic().await;

    let names = |q: &str| {
        let query = ArtistIdentityGroupQuery {
            q: Some(q.into()),
            library: None,
            offset: None,
            limit: None,
        };
        let artists = library.hito.artist_catalog.clone();
        let actor = library.actor.clone();
        async move {
            artists
                .artist_identity_groups(&actor, query)
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|group| group.name.value)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(names("nova quartet").await, vec![CREDIT, LONGER]);
    assert_eq!(names("  NOVA   quartet ").await, vec![CREDIT, LONGER]);
    assert_eq!(names("a nova quartet").await, vec![LONGER]);
    assert_eq!(
        names("nova").await,
        vec![LONGER, CREDIT],
        "With no exact match the alphabetical order stays"
    );
}
