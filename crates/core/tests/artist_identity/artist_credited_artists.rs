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
    json!({"name": name, "role": "main", "join": join, "page_url": page})
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

async fn credit_ids(library: &NameLibrary) -> Vec<(String, String, i64)> {
    library
        .hito
        .catalog
        .artists(&library.actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|artist| (artist.id, artist.name, artist.recording_count))
        .collect()
}

#[tokio::test]
async fn a_refresh_with_a_list_keeps_the_recording_on_its_existing_text_credit() {
    let library = NameLibrary::prepare(&[
        ("Ada, Bo", "Song 0", 100_000),
        ("Ada, Bo", "Song 1", 100_001),
        ("Bo, Ada", "Song 2", 100_002),
    ])
    .await;
    let before = credit_ids(&library).await;
    assert_eq!(before.len(), 2);
    let existing = before.iter().find(|credit| credit.1 == "Ada, Bo").unwrap();
    library
        .reimport(
            "refresh-0",
            &[(
                0,
                "Ada, Bo",
                "Song 0",
                100_000,
                structured(
                    vec![
                        main_artist("Ada", Some(", "), None),
                        main_artist("Bo", None, None),
                    ],
                    &[],
                ),
            )],
        )
        .await;
    assert_eq!(
        credit_ids(&library).await,
        before,
        "The refreshed recording stays on the artist it had and nothing is split"
    );
    library
        .reimport(
            "refresh-1",
            &[(
                1,
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
            )],
        )
        .await;
    let after = credit_ids(&library).await;
    assert!(
        after.contains(&(existing.0.clone(), "Ada, Bo".into(), 2)),
        "A reordered refresh joins the same artist under its label: {after:?}"
    );
    assert_eq!(after.len(), 2, "The text-only credit stays separate");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_new_credit_from_a_refresh_asks_for_a_matching_walk_and_a_feature_does_not_adopt() {
    let library = NameLibrary::prepare(&[("Ada feat. Cy", "Song 0", 100_000)]).await;
    library.execute("DELETE FROM artist_matching_jobs").await;
    library
        .reimport(
            "refresh-feature",
            &[(
                0,
                "Ada feat. Cy",
                "Song 0",
                100_000,
                structured(
                    vec![
                        main_artist("Ada", Some(" feat. "), None),
                        featured("Cy", None),
                    ],
                    &[],
                ),
            )],
        )
        .await;
    assert_eq!(
        credits(&library).await,
        [("Ada".to_owned(), 1)],
        "A feature credit is the main artist's, not the text credit's"
    );
    assert_eq!(library.rows("artist_matching_jobs").await, 1);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn names_fold_by_trimmed_unicode_lower_case_and_nothing_else() {
    let library = NameLibrary::prepare_with_evidence(&[
        (
            "\u{c9}mile & Bo",
            "Song 0",
            100_000,
            structured(
                vec![
                    main_artist("\u{c9}mile", Some(" & "), None),
                    main_artist("Bo", None, None),
                ],
                &[],
            ),
        ),
        (
            "bo & \u{e9}mile",
            "Song 1",
            100_001,
            structured(
                vec![
                    main_artist("\u{a0}bo ", Some(" & "), None),
                    main_artist("\u{e9}MILE", None, None),
                ],
                &[],
            ),
        ),
        (
            "Bo & Emile",
            "Song 2",
            100_002,
            structured(
                vec![
                    main_artist("Bo", Some(" & "), None),
                    main_artist("Emile", None, None),
                ],
                &[],
            ),
        ),
        (
            "Bo  &  \u{e9}mile",
            "Song 3",
            100_003,
            structured(
                vec![
                    main_artist("Bo", Some("  &  "), None),
                    main_artist("\u{e9}mi  le", None, None),
                ],
                &[],
            ),
        ),
    ])
    .await;
    let found = credits(&library).await;
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found.contains(&("\u{c9}mile & Bo".to_owned(), 2)),
        "{found:?}"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_list_of_sixteen_widest_names_is_kept_whole_or_not_at_all() {
    let names: Vec<String> = (0..16)
        .map(|index| format!("{}{index:02}", "\u{130}".repeat(127)))
        .collect();
    assert!(names.iter().all(|name| name.len() == 256));
    let artists: Vec<Value> = names
        .iter()
        .map(|name| main_artist(name, Some(" & "), None))
        .collect();
    let library = NameLibrary::prepare_with_evidence(&[(
        "Sixteen",
        "Song 0",
        100_000,
        structured(artists, &[]),
    )])
    .await;
    assert_eq!(library.rows("recording_credit_observations").await, 1);
    assert_eq!(
        library.rows("recording_credited_artist_observations").await,
        16
    );
    assert_eq!(credits(&library).await.len(), 1);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn more_identifying_pages_than_the_page_limit_identify_nothing() {
    let pages: Vec<String> = (0..9)
        .map(|index| format!("https://open.spotify.com/artist/{index:022}"))
        .collect();
    let artists: Vec<Value> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| main_artist(&format!("Artist {index}"), Some(" & "), Some(page)))
        .collect();
    let library = NameLibrary::prepare_with_evidence(&[(
        "Nine",
        "Song 0",
        100_000,
        structured(artists, &[]),
    )])
    .await;
    assert_eq!(library.rows("recording_artist_url_observations").await, 0);
    assert_eq!(
        library.rows("recording_credited_artist_observations").await,
        9
    );
    library.origin.shutdown().await;
}
