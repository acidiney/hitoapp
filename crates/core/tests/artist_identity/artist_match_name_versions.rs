#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use library::{CREDIT, NOVA, NameLibrary, nova_discography};

fn exact_artist() -> Reply {
    Reply::Candidates(vec![(NOVA.into(), CREDIT.into())])
}

async fn run(
    tracks: &[(&str, u64)],
    discography: &[(&str, Option<u64>)],
) -> (NameLibrary, hito_core::ArtistProviderMatchReceipt) {
    let entries: Vec<(&str, &str, u64)> = tracks
        .iter()
        .map(|(title, length)| (CREDIT, *title, *length))
        .collect();
    let library = NameLibrary::prepare(&entries).await;
    library.musicbrainz.search(CREDIT, exact_artist());
    library
        .musicbrainz
        .recordings_of(NOVA, nova_discography(discography));
    let receipt = library.run().await;
    (library, receipt)
}

#[tokio::test]
async fn version_titles_follow_the_artist_through_the_base_title_or_the_same_qualifier() {
    let (library, receipt) = run(
        &[
            ("Morning Light", 201_000),
            ("Second Wind", 187_500),
            ("Morning Light (Acoustic)", 196_000),
            ("Harbor Lights - Live", 250_000),
            ("Second Wind [Remastered 2011]", 188_000),
            ("Quiet Rooms (feat. Guest)", 150_500),
        ],
        &[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Morning Light - Acoustic", Some(190_000)),
            ("Harbor Lights (Live)", Some(255_000)),
            ("Quiet Rooms", Some(150_000)),
        ],
    )
    .await;
    assert_eq!((receipt.matched, receipt.uncertain), (6, 0));
    for recording in &library.recordings {
        assert!(library.group_of(recording).await.is_some());
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_remix_matches_a_remix_of_the_same_title_and_never_the_original() {
    let tracks = [
        ("Morning Light", 201_000),
        ("Second Wind", 187_500),
        ("Second Wind - Alex Remix", 200_000),
    ];
    let (library, receipt) = run(
        &tracks,
        &[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Second Wind (Other Remix)", Some(205_000)),
        ],
    )
    .await;
    assert_eq!((receipt.matched, receipt.uncertain), (3, 0));
    library.origin.shutdown().await;

    let (library, receipt) = run(
        &tracks,
        &[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Second Wind", Some(200_000)),
        ],
    )
    .await;
    assert_eq!(
        (receipt.matched, receipt.uncertain),
        (2, 1),
        "The original recording is not the remix, whatever its length"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn the_wider_length_applies_only_when_both_titles_carry_the_same_qualifier() {
    let (library, receipt) = run(
        &[
            ("Morning Light", 201_000),
            ("Second Wind", 187_500),
            ("Harbor Lights - Live", 250_000),
            ("Quiet Rooms", 150_000),
        ],
        &[
            ("Morning Light", Some(201_000)),
            ("Second Wind", Some(187_500)),
            ("Harbor Lights", Some(241_000)),
            ("Quiet Rooms", Some(148_001)),
        ],
    )
    .await;
    assert_eq!(
        (receipt.matched, receipt.uncertain),
        (3, 1),
        "Nine seconds apart is too far for the base title; a plain title stays within two seconds"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn titles_of_other_songs_that_share_words_do_not_corroborate() {
    let (library, receipt) = run(
        &[
            ("Photograph", 258_000),
            ("Perfect", 263_000),
            ("Perfect Duet", 259_000),
        ],
        &[
            ("Photographs", Some(258_000)),
            ("Perfect Symphony", Some(263_000)),
            ("Perfect (Duet)", Some(259_000)),
        ],
    )
    .await;
    assert_eq!((receipt.matched, receipt.uncertain), (0, 3));
    library.origin.shutdown().await;
}
