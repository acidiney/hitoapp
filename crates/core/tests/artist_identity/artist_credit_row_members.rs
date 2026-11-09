#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::{Credentials, CreditRole, Search};
use library::{Evidence, NameLibrary};
use serde_json::{Value, json};
use uuid::Uuid;

fn main_artist(name: &str, join: Option<&str>) -> Value {
    match join {
        Some(join) => json!({"name": name, "role": "main", "join": join}),
        None => json!({"name": name, "role": "main"}),
    }
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
            evidence(vec![
                main_artist("Ada", Some(" feat. ")),
                json!({"name": "Cy", "role": "featured"}),
            ]),
        ),
        ("Ada and Bo", "Song 3", 100_003, Evidence::default()),
        ("Solo", "Song 4", 100_004, Evidence::default()),
    ])
    .await
}

fn members(recording: &hito_core::Recording) -> Option<Vec<(String, CreditRole, Option<String>)>> {
    recording.credited_artists.as_ref().map(|members| {
        members
            .iter()
            .map(|member| (member.name.clone(), member.role, member.join.clone()))
            .collect()
    })
}

async fn listed(library: &NameLibrary, title: &str) -> hito_core::Recording {
    library
        .hito
        .catalog
        .recordings(&library.actor, Search::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|recording| recording.title == title)
        .unwrap()
}

#[tokio::test]
async fn rows_carry_their_members_in_credit_order_with_featured_artists_last() {
    let library = collaboration_library().await;
    let ada_bo = listed(&library, "Song 0").await;
    assert_eq!(
        members(&ada_bo).unwrap(),
        [
            ("Ada".to_owned(), CreditRole::Main, Some(" & ".to_owned())),
            ("Bo".to_owned(), CreditRole::Main, None),
        ]
    );
    let bo_ada = listed(&library, "Song 1").await;
    assert_eq!(
        members(&bo_ada).unwrap(),
        [
            ("Bo".to_owned(), CreditRole::Main, Some(", ".to_owned())),
            ("Ada".to_owned(), CreditRole::Main, None),
        ]
    );
    let featuring = listed(&library, "Song 2").await;
    assert_eq!(
        members(&featuring).unwrap(),
        [
            (
                "Ada".to_owned(),
                CreditRole::Main,
                Some(" feat. ".to_owned())
            ),
            ("Cy".to_owned(), CreditRole::Featured, None),
        ]
    );
    let bo_id: String = library
        .scalar("SELECT id FROM account_artists WHERE name='Bo'")
        .await;
    assert_eq!(ada_bo.credited_artists.unwrap()[1].id, bo_id);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn text_only_and_solo_recordings_stay_as_they_were() {
    let library = collaboration_library().await;
    for title in ["Song 3", "Song 4"] {
        let recording = listed(&library, title).await;
        assert!(recording.credited_artists.is_none(), "{title}");
        let wire = serde_json::to_value(&recording).unwrap();
        assert!(wire.get("credited_artists").is_none(), "{title}");
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn every_row_reader_attaches_members() {
    let library = collaboration_library().await;
    let id = Uuid::parse_str(&listed(&library, "Song 0").await.id).unwrap();
    let selected = library
        .hito
        .catalog
        .recordings_by_ids(&library.actor, &[id])
        .await
        .unwrap();
    assert_eq!(selected[0].credited_artists.as_ref().unwrap().len(), 2);
    let ada: String = library
        .scalar("SELECT id FROM account_artists WHERE name='Ada'")
        .await;
    let page = library
        .hito
        .catalog
        .artist(&library.actor, &ada, Default::default())
        .await
        .unwrap();
    assert!(
        page.recordings
            .items
            .iter()
            .all(|recording| recording.credited_artists.is_some() || recording.title == "Solo")
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn another_account_never_receives_members_of_a_recording_it_cannot_read() {
    let library = collaboration_library().await;
    let id = Uuid::parse_str(&listed(&library, "Song 0").await.id).unwrap();
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
    assert!(
        library
            .hito
            .catalog
            .readable_recordings(&stranger, &[id])
            .await
            .unwrap()
            .is_empty()
    );
    let own = library
        .hito
        .catalog
        .recordings(&stranger, Search::default())
        .await
        .unwrap();
    assert!(own.items.is_empty());
    library.origin.shutdown().await;
}
