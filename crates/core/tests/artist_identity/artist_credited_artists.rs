#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{Action, Browse, CreditRole, MetadataHistoryQuery};
use library::{Evidence, NOVA, NameLibrary};
use serde_json::{Value, json};
use uuid::Uuid;

const NOVA_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000011";
const GUEST_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000012";
const OTHER_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000013";

fn main_artist(name: &str, join: Option<&str>, page: Option<&str>) -> Value {
    json!({"name": name, "role": "main", "join": join, "page_url": page, "provider_artist_id": name})
}

fn featured(name: &str, page: Option<&str>) -> Value {
    json!({"name": name, "role": "featured", "page_url": page})
}

fn structured(artists: Vec<Value>, pages: &[&str]) -> Evidence {
    Evidence {
        isrc: None,
        artist_urls: pages.iter().map(|page| (*page).to_owned()).collect(),
        artists: artists
            .into_iter()
            .map(|artist| {
                let Value::Object(mut fields) = artist else {
                    unreachable!()
                };
                fields.retain(|_, value| !value.is_null());
                Value::Object(fields)
            })
            .collect(),
    }
}

async fn credits(library: &NameLibrary) -> Vec<(String, i64)> {
    library
        .hito
        .catalog
        .artists(&library.actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|artist| (artist.name, artist.recording_count))
        .collect()
}

async fn collaboration_library() -> NameLibrary {
    NameLibrary::prepare_with_evidence(&[
        (
            "Ada & Bo",
            "Song 0",
            100_000,
            structured(
                vec![
                    main_artist("Ada", Some(" & "), None),
                    main_artist("Bo", None, None),
                ],
                &[],
            ),
        ),
        (
            "Bo, Ada",
            "Song 1",
            100_001,
            structured(
                vec![
                    main_artist("Bo", Some(", "), None),
                    main_artist("Ada", None, None),
                ],
                &[],
            ),
        ),
        (
            "Ada feat. Cy",
            "Song 2",
            100_002,
            structured(
                vec![
                    main_artist("Ada", Some(" feat. "), None),
                    featured("Cy", None),
                ],
                &[],
            ),
        ),
        ("Ada and Bo", "Song 3", 100_003, Evidence::default()),
    ])
    .await
}

#[tokio::test]
async fn reordered_collaborations_share_one_credit_and_a_feature_credits_only_the_main_artist() {
    let library = collaboration_library().await;
    assert_eq!(
        credits(&library).await,
        [
            ("Ada".to_owned(), 1),
            ("Ada & Bo".to_owned(), 2),
            ("Ada and Bo".to_owned(), 1)
        ],
        "Reordered members form one credit, the feature leaves the main artist alone, and a credit written only as text stays separate"
    );
    let shown: Vec<String> = library
        .hito
        .catalog
        .recordings(&library.actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|recording| recording.artist)
        .collect();
    for text in ["Ada & Bo", "Bo, Ada", "Ada feat. Cy", "Ada and Bo"] {
        assert!(
            shown.contains(&text.to_owned()),
            "{text} stays the displayed artist: {shown:?}"
        );
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn credited_artists_are_read_in_order_and_link_to_artists_the_account_has() {
    let library = collaboration_library().await;
    let ada = library
        .hito
        .catalog
        .artists(&library.actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|artist| artist.name == "Ada")
        .unwrap()
        .id;
    let read = |index: usize| {
        let library = &library;
        async move {
            library
                .hito
                .catalog
                .recording_metadata(
                    &library.actor,
                    &library.recordings[index],
                    MetadataHistoryQuery::default(),
                )
                .await
                .unwrap()
                .credited_artists
        }
    };
    let reordered = read(1).await;
    let seen: Vec<_> = reordered
        .iter()
        .map(|artist| {
            (
                artist.name.as_str(),
                artist.role,
                artist.join.as_deref(),
                artist.artist_id.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("Bo", CreditRole::Main, Some(", "), None),
            ("Ada", CreditRole::Main, None, Some(ada.as_str()))
        ]
    );
    let feature = read(2).await;
    assert_eq!(feature[1].name, "Cy");
    assert_eq!(feature[1].role, CreditRole::Featured);
    assert!(read(3).await.is_empty(), "Text-only credits carry no list");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_personal_artist_edit_wins_over_the_structured_credit() {
    let library = collaboration_library().await;
    let before = credits(&library).await;
    hito_core::test_support::support::synchronize(
        &library.hito,
        &library.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&library.recordings[0]).unwrap(),
            artist: "Personally Edited".into(),
        }],
    )
    .await;
    let after = credits(&library).await;
    assert!(
        after.contains(&("Personally Edited".to_owned(), 1)),
        "{after:?}"
    );
    assert!(
        after.contains(&("Ada & Bo".to_owned(), 1)),
        "{after:?} from {before:?}"
    );
    let metadata = library
        .hito
        .catalog
        .recording_metadata(
            &library.actor,
            &library.recordings[0],
            MetadataHistoryQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(metadata.artist.value, "Personally Edited");
    assert!(
        metadata.credited_artists.is_empty(),
        "The edited text is no longer the provider's credit"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn only_the_main_artists_pages_can_identify_a_credit() {
    let library = NameLibrary::prepare_with_evidence(&[
        (
            "Nova feat. Guest",
            "Song 0",
            100_000,
            structured(
                vec![
                    main_artist("Nova", Some(" feat. "), Some(NOVA_PAGE)),
                    featured("Guest", Some(GUEST_PAGE)),
                ],
                &[NOVA_PAGE, GUEST_PAGE],
            ),
        ),
        (
            "Nova & Other",
            "Song 1",
            100_001,
            structured(
                vec![
                    main_artist("Nova", Some(" & "), Some(NOVA_PAGE)),
                    main_artist("Other", None, Some(OTHER_PAGE)),
                ],
                &[NOVA_PAGE, OTHER_PAGE],
            ),
        ),
        (
            "Nova & Plain",
            "Song 2",
            100_002,
            structured(
                vec![
                    main_artist("Nova", Some(" & "), Some(NOVA_PAGE)),
                    main_artist("Plain", Some(" feat. "), None),
                    featured("Guest", Some(GUEST_PAGE)),
                ],
                &[GUEST_PAGE],
            ),
        ),
    ])
    .await;
    library
        .musicbrainz
        .url(NOVA_PAGE, Reply::Artists(vec![NOVA.into()]));
    library
        .musicbrainz
        .url(GUEST_PAGE, Reply::Artists(vec![library::NAMESAKE.into()]));
    library
        .musicbrainz
        .url(OTHER_PAGE, Reply::Artists(vec![library::TRIBUTE.into()]));
    let receipt = library.run().await;
    assert_eq!(receipt.matched, 1, "{receipt:?}");
    assert!(library.group_of(&library.recordings[0]).await.is_some());
    assert!(library.group_of(&library.recordings[1]).await.is_none());
    assert!(library.group_of(&library.recordings[2]).await.is_none());
    let lookups: Vec<String> = library
        .musicbrainz
        .requests()
        .into_iter()
        .filter(|request| request.starts_with("/ws/2/url?"))
        .collect();
    assert_eq!(lookups.len(), 1, "{lookups:?}");
    assert!(lookups[0].contains("0000000000000000000011"), "{lookups:?}");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_collaboration_keeps_the_wording_and_order_it_was_first_seen_with() {
    let library = NameLibrary::prepare_with_evidence(&[
        (
            "Simon & Garfunkel",
            "Song 0",
            100_000,
            structured(
                vec![
                    main_artist("Simon", Some(" & "), None),
                    main_artist("Garfunkel", None, None),
                ],
                &[],
            ),
        ),
        (
            "Garfunkel & Simon",
            "Song 1",
            100_001,
            structured(
                vec![
                    main_artist("Garfunkel", Some(" & "), None),
                    main_artist("Simon", None, None),
                ],
                &[],
            ),
        ),
        (
            "SIMON & garfunkel",
            "Song 2",
            100_002,
            structured(
                vec![
                    main_artist("SIMON", Some(" & "), None),
                    main_artist("garfunkel", None, None),
                ],
                &[],
            ),
        ),
    ])
    .await;
    assert_eq!(
        credits(&library).await,
        [("Simon & Garfunkel".to_owned(), 3)],
        "Every order and casing is one credit, labelled as first seen"
    );
    library.origin.shutdown().await;
}
