#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
pub(super) mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    Actor, ArtistReviewItem, ArtistReviewQueueQuery, ConfirmArtistReviewCandidate,
    DismissArtistReviewCandidates, Error,
};
use library::{CREDIT, NAMESAKE, NOVA, NameLibrary, TRIBUTE, nova_discography};
use uuid::Uuid;

pub(super) const EDITED: &str = "nova quartet";

pub(super) fn candidates(entries: &[(&str, &str)]) -> Reply {
    Reply::Candidates(
        entries
            .iter()
            .map(|(id, name)| ((*id).into(), (*name).into()))
            .collect(),
    )
}

pub(super) async fn queue_of(library: &NameLibrary, actor: &Actor) -> Vec<ArtistReviewItem> {
    library
        .hito
        .artist_catalog
        .artist_review_queue(actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap()
        .items
}

pub(super) async fn queue(library: &NameLibrary) -> Vec<ArtistReviewItem> {
    queue_of(library, &library.actor).await
}

pub(super) async fn confirm_as(
    library: &NameLibrary,
    actor: &Actor,
    credit: &str,
    artist: &str,
) -> hito_core::Result<hito_core::ArtistReviewConfirmation> {
    library
        .hito
        .artist_catalog
        .confirm_artist_review_candidate(
            actor,
            credit,
            ConfirmArtistReviewCandidate {
                request_id: Uuid::new_v4(),
                provider_artist_id: artist.into(),
                after: None,
            },
        )
        .await
}

pub(super) async fn dismiss(
    library: &NameLibrary,
    credit: &str,
    artist: Option<&str>,
) -> hito_core::Result<hito_core::ArtistReviewDismissal> {
    library
        .hito
        .artist_catalog
        .dismiss_artist_review_candidates(
            &library.actor,
            credit,
            DismissArtistReviewCandidates {
                provider_artist_id: artist.map(Into::into),
            },
        )
        .await
}

#[tokio::test]
async fn an_undecided_credit_without_candidates_is_listed_after_the_credits_that_have_some() {
    let entries: Vec<(&str, &str, u64)> = vec![
        ("Alpha Band", "One", 200_000),
        ("Alpha Band", "Two", 210_000),
        ("Zed Duo", "Three", 220_000),
        ("Zed Duo", "Four", 230_000),
        ("Zed Duo", "Five", 240_000),
    ];
    let library = NameLibrary::prepare(&entries).await;
    library.musicbrainz.search(
        "Alpha Band",
        candidates(&[(NOVA, "Alpha Band"), (NAMESAKE, "Alpha Band")]),
    );
    library
        .musicbrainz
        .search("Zed Duo", candidates(&[(TRIBUTE, "Zed Duo Tribute")]));
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(&[]));
    library
        .musicbrainz
        .recordings_of(NAMESAKE, nova_discography(&[]));
    library.run().await;
    let page = library
        .hito
        .artist_catalog
        .artist_review_queue(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap();
    assert_eq!(page.total, 2);
    let shape: Vec<(&str, i64, bool, usize)> = page
        .items
        .iter()
        .map(|item| {
            (
                item.name.as_str(),
                item.undecided_tracks,
                item.needs_search,
                item.candidates.len(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![("Alpha Band", 2, false, 2), ("Zed Duo", 3, true, 0)],
        "Credits with candidates come first even when the other credit is larger"
    );
    let paged = library
        .hito
        .artist_catalog
        .artist_review_queue(
            &library.actor,
            ArtistReviewQueueQuery {
                offset: Some(1),
                limit: Some(1),
            },
        )
        .await
        .unwrap();
    assert_eq!((paged.total, paged.items[0].name.as_str()), (2, "Zed Duo"));
    let asked = library.musicbrainz.requests().len();
    queue(&library).await;
    assert_eq!(library.musicbrainz.requests().len(), asked);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_credit_with_nothing_to_confirm_or_dismiss_stays_until_the_account_resolves_it() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(TRIBUTE, "Nova Quartet Tribute")]));
    library.run().await;
    let items = queue(&library).await;
    assert_eq!(items.len(), 1);
    assert_eq!(
        (
            items[0].name.as_str(),
            items[0].undecided_tracks,
            items[0].needs_search
        ),
        (CREDIT, 4, true)
    );
    let credit = items[0].artist_id.clone();
    assert!(
        items[0]
            .search_recording_id
            .as_ref()
            .is_some_and(|id| library.recordings.contains(id)),
        "The search opens one of the credit's undecided tracks"
    );
    assert!(matches!(
        confirm_as(&library, &library.actor, &credit, NOVA).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        dismiss(&library, &credit, Some(NOVA)).await,
        Err(Error::NotFound)
    ));
    assert_eq!(dismiss(&library, &credit, None).await.unwrap().dismissed, 0);
    assert_eq!(queue(&library).await.len(), 1, "Nothing was dismissed");
    assert!(
        library
            .hito
            .artist_catalog
            .dismissed_artist_reviews(&library.actor, ArtistReviewQueueQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_decided_credit_is_not_listed_for_search() {
    let library = NameLibrary::nova().await;
    library
        .musicbrainz
        .search(CREDIT, candidates(&[(NOVA, CREDIT)]));
    library.musicbrainz.recordings_of(
        NOVA,
        nova_discography(&[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Harbor Lights", Some(243_000)),
            ("Quiet Rooms", Some(150_000)),
        ]),
    );
    library.run().await;
    assert!(queue(&library).await.is_empty());
    library.origin.shutdown().await;
}
