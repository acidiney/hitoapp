#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::{Action, Browse, Credentials};
use library::{Evidence, NameLibrary};
use serde_json::{Value, json};
use uuid::Uuid;

fn main_artist(name: &str, join: Option<&str>) -> Value {
    match join {
        Some(join) => json!({"name": name, "role": "main", "join": join}),
        None => json!({"name": name, "role": "main"}),
    }
}

fn featured(name: &str) -> Value {
    json!({"name": name, "role": "featured"})
}

fn evidence(artists: Vec<Value>) -> Evidence {
    Evidence {
        isrc: None,
        artist_urls: Vec::new(),
        artists,
    }
}

async fn collaboration_library() -> NameLibrary {
    NameLibrary::prepare_with_evidence(&[
        (
            "Ada & Bo",
            "Song 0",
            100_000,
            evidence(vec![
                main_artist("Ada", Some(" & ")),
                main_artist("Bo", None),
            ]),
        ),
        (
            "Bo, Ada",
            "Song 1",
            100_001,
            evidence(vec![
                main_artist("Bo", Some(", ")),
                main_artist("Ada", None),
            ]),
        ),
        (
            "Ada feat. Cy",
            "Song 2",
            100_002,
            evidence(vec![main_artist("Ada", Some(" feat. ")), featured("Cy")]),
        ),
        ("Ada and Bo", "Song 3", 100_003, Evidence::default()),
    ])
    .await
}

struct Page {
    recordings: Vec<String>,
    featured: Vec<String>,
    recording_count: i64,
}

async fn page(library: &NameLibrary, name: &str) -> Option<Page> {
    let listed = library
        .hito
        .catalog
        .artists(&library.actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|artist| artist.name == name)?;
    let detail = library
        .hito
        .catalog
        .artist(&library.actor, &listed.id, Browse::default())
        .await
        .unwrap();
    let titles = |recordings: hito_core::Page<hito_core::Recording>| {
        let mut titles: Vec<String> = recordings.items.into_iter().map(|r| r.title).collect();
        titles.sort();
        titles
    };
    Some(Page {
        recordings: titles(detail.recordings),
        featured: titles(detail.featured_recordings),
        recording_count: detail.artist.recording_count,
    })
}

#[tokio::test]
async fn every_main_artist_of_a_collaboration_lists_it_whatever_the_order() {
    let library = collaboration_library().await;
    let ada = page(&library, "Ada").await.unwrap();
    assert_eq!(ada.recordings, ["Song 0", "Song 1", "Song 2"]);
    assert_eq!(ada.recording_count, 3);
    let bo = page(&library, "Bo").await.unwrap();
    assert_eq!(bo.recordings, ["Song 0", "Song 1"]);
    assert_eq!(bo.recording_count, 2);
    let combined = page(&library, "Ada & Bo").await.unwrap();
    assert_eq!(combined.recordings, ["Song 0", "Song 1"]);
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
    for text in ["Ada & Bo", "Bo, Ada"] {
        assert!(shown.contains(&text.to_owned()), "{shown:?}");
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_featured_artist_lists_the_recording_as_appearing_on_without_a_main_credit() {
    let library = collaboration_library().await;
    let cy = page(&library, "Cy").await.unwrap();
    assert!(cy.recordings.is_empty());
    assert_eq!(cy.recording_count, 0);
    assert_eq!(cy.featured, ["Song 2"]);
    let ada = page(&library, "Ada").await.unwrap();
    assert!(ada.featured.is_empty());
    let credit: String = library
        .scalar(
            "SELECT a.name FROM account_recording_credits c JOIN account_artists a ON a.id=c.artist_id WHERE c.recording_id=(SELECT id FROM recordings WHERE title='Song 2')",
        )
        .await;
    assert_eq!(credit, "Ada");
    library.origin.shutdown().await;
}

#[tokio::test]
async fn text_only_credits_are_never_split() {
    let library = collaboration_library().await;
    assert!(page(&library, "Ada and Bo").await.unwrap().recordings == ["Song 3"]);
    assert_eq!(
        library.scalar("SELECT count(*) FROM account_recording_credit_members m JOIN recordings r ON r.id=m.recording_id WHERE r.title='Song 3'").await,
        "0"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_personal_artist_edit_removes_the_recording_from_member_pages() {
    let library = collaboration_library().await;
    hito_core::test_support::support::synchronize(
        &library.hito,
        &library.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&library.recordings[0]).unwrap(),
            artist: "Personally Edited".into(),
        }],
    )
    .await;
    assert_eq!(
        page(&library, "Ada").await.unwrap().recordings,
        ["Song 1", "Song 2"]
    );
    assert_eq!(page(&library, "Bo").await.unwrap().recordings, ["Song 1"]);
    assert_eq!(
        page(&library, "Personally Edited")
            .await
            .unwrap()
            .recordings,
        ["Song 0"]
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn another_account_sees_no_member_pages() {
    let library = collaboration_library().await;
    let ada: String = library
        .scalar("SELECT id FROM account_artists WHERE name='Ada'")
        .await;
    let credentials = || Credentials {
        username: "stranger".into(),
        password: "another account password".into(),
    };
    library
        .hito
        .identity
        .create_user(&library.actor, credentials())
        .await
        .unwrap();
    let session = library.hito.identity.login(credentials()).await.unwrap();
    let stranger = library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let listed = library
        .hito
        .catalog
        .artists(&stranger, Browse::default())
        .await
        .unwrap();
    assert_eq!(listed.total, 0);
    assert!(
        library
            .hito
            .catalog
            .artist(&stranger, &ada, Browse::default())
            .await
            .is_err()
    );
    assert_eq!(
        library
            .scalar("SELECT count(*) FROM account_recording_credit_members WHERE user_id=(SELECT id FROM users WHERE username='stranger')")
            .await,
        "0"
    );
    library.origin.shutdown().await;
}
