#[allow(dead_code)]
#[path = "../fixtures/artist_name_library.rs"]
mod library;

use hito_core::test_support::musicbrainz::Reply;
use hito_core::{
    Action, ArtistAssociationEvidence, ArtistReviewQueueQuery, CreditRole, MetadataHistoryQuery,
    RecordingArtistIdentityState, ResetRecordingArtistIdentity,
};
use library::{Evidence, NameLibrary, nova_discography};
use serde_json::{Value, json};
use uuid::Uuid;

const ADA: &str = "a0000000-0000-4000-8000-00000000000a";
const BO: &str = "b0000000-0000-4000-8000-00000000000b";
const DUO: &str = "d0000000-0000-4000-8000-00000000000d";

const ADA_SONGS: [(&str, u64); 2] = [("Ada Morning", 201_000), ("Ada Evening", 187_500)];
const BO_SONGS: [(&str, u64); 2] = [("Bo Harbor", 243_000), ("Bo Rooms", 150_000)];
const DUET_SONGS: [(&str, u64); 3] = [
    ("Duet Lights", 211_000),
    ("Duet Wind", 176_000),
    ("Duet Rain", 199_000),
];

fn listed(names: &[&str]) -> Evidence {
    let artists: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if index + 1 < names.len() {
                json!({"name": name, "role": "main", "join": " & "})
            } else {
                json!({"name": name, "role": "main"})
            }
        })
        .collect();
    Evidence {
        isrc: None,
        artist_urls: Vec::new(),
        artists,
    }
}

fn searched(name: &str) -> String {
    name.replace('&', "\\&")
}

fn known(library: &NameLibrary, name: &str, id: &str, songs: &[(&str, u64)]) {
    library.musicbrainz.search(
        &searched(name),
        Reply::Candidates(vec![(id.into(), name.into())]),
    );
    let discography: Vec<(&str, Option<u64>)> = songs
        .iter()
        .map(|(title, length)| (*title, Some(*length)))
        .collect();
    library
        .musicbrainz
        .recordings_of(id, nova_discography(&discography));
}

fn unknown(library: &NameLibrary, name: &str) {
    library
        .musicbrainz
        .search(&searched(name), Reply::Candidates(vec![]));
}

async fn prepare(
    duet: &str,
    duet_evidence: Evidence,
    solos: &[(&str, &[(&str, u64)])],
) -> NameLibrary {
    let mut entries: Vec<(&str, &str, u64, Evidence)> = Vec::new();
    for (name, songs) in solos {
        for (title, length) in *songs {
            entries.push((*name, *title, *length, Evidence::default()));
        }
    }
    for (title, length) in DUET_SONGS {
        entries.push((duet, title, length, duet_evidence.clone()));
    }
    NameLibrary::prepare_with_evidence(&entries).await
}

fn recording_of(library: &NameLibrary, position: usize) -> String {
    library.recordings[position].clone()
}

fn duets(library: &NameLibrary) -> Vec<String> {
    library.recordings[library.recordings.len() - DUET_SONGS.len()..].to_vec()
}

async fn artist(library: &NameLibrary, name: &str) -> String {
    library
        .scalar(&format!(
            "SELECT id FROM account_artists WHERE name='{name}' AND user_id=(SELECT id FROM users WHERE username='admin')"
        ))
        .await
}

async fn member_group(library: &NameLibrary, recording: &str, name: &str) -> Option<String> {
    let member = artist(library, name).await;
    match library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(&library.actor, recording, &member)
        .await
        .ok()?
        .state
    {
        RecordingArtistIdentityState::Selected(selection) => Some(selection.group_id),
        _ => None,
    }
}

async fn member_names(library: &NameLibrary, recording: &str) -> Vec<String> {
    library
        .hito
        .catalog
        .recording_metadata(&library.actor, recording, MetadataHistoryQuery::default())
        .await
        .unwrap()
        .credited_artists
        .into_iter()
        .filter(|member| member.artist_id.is_some())
        .map(|member| member.name)
        .collect()
}

async fn member_rows(library: &NameLibrary, recording: &str) -> i64 {
    library
        .scalar(&format!(
            "SELECT count(*) FROM account_recording_credit_members WHERE recording_id='{recording}'"
        ))
        .await
        .parse()
        .unwrap()
}

async fn member_evidence(
    library: &NameLibrary,
    recording: &str,
    name: &str,
) -> Vec<ArtistAssociationEvidence> {
    let member = artist(library, name).await;
    library
        .associations(recording)
        .await
        .into_iter()
        .filter(|association| association.artist_id == member)
        .map(|association| association.evidence)
        .collect()
}

async fn queued(library: &NameLibrary) -> Vec<String> {
    library
        .hito
        .artist_catalog
        .artist_review_queue(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.name)
        .collect()
}

#[tokio::test]
async fn a_text_credit_with_no_artist_of_its_whole_name_takes_its_members_from_their_own_credits() {
    let library = prepare(
        "Ada & Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    library.run().await;
    let ada = library.group_of(&recording_of(&library, 0)).await.unwrap();
    let bo = library.group_of(&recording_of(&library, 2)).await.unwrap();
    for duet in duets(&library) {
        assert_eq!(member_names(&library, &duet).await, ["Ada", "Bo"]);
        assert_eq!(
            member_group(&library, &duet, "Ada").await.as_ref(),
            Some(&ada)
        );
        assert_eq!(
            member_group(&library, &duet, "Bo").await.as_ref(),
            Some(&bo)
        );
        assert_eq!(
            member_evidence(&library, &duet, "Ada").await,
            [ArtistAssociationEvidence::MatchedMemberSoloCredit]
        );
        assert_eq!(library.group_of(&duet).await, None);
    }
    let joins: Vec<Option<String>> = library
        .hito
        .catalog
        .recording_metadata(
            &library.actor,
            &duets(&library)[0],
            MetadataHistoryQuery::default(),
        )
        .await
        .unwrap()
        .credited_artists
        .into_iter()
        .map(|member| member.join)
        .collect();
    assert_eq!(joins, [Some(" & ".to_owned()), None]);
    assert!(
        !queued(&library).await.contains(&"Ada & Bo".to_owned()),
        "A credit whose members are all identified leaves the review queue"
    );
    library.run().await;
    assert_eq!(member_rows(&library, &duets(&library)[0]).await, 2);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_text_credit_with_a_member_the_library_cannot_vouch_for_stays_whole() {
    let library = prepare("Ada & Zed", Evidence::default(), &[("Ada", &ADA_SONGS)]).await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    unknown(&library, "Ada & Zed");
    library.run().await;
    assert!(library.group_of(&recording_of(&library, 0)).await.is_some());
    for duet in duets(&library) {
        assert_eq!(member_rows(&library, &duet).await, 0);
        assert!(library.associations(&duet).await.is_empty());
    }
    assert_eq!(library.rows("account_credit_splits").await, 0);
    assert!(queued(&library).await.contains(&"Ada & Zed".to_owned()));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_duo_whose_whole_name_is_an_artist_is_never_split() {
    let library = prepare(
        "Ada & Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    known(&library, "Ada & Bo", DUO, &DUET_SONGS);
    library.run().await;
    for duet in duets(&library) {
        assert!(library.group_of(&duet).await.is_some());
        assert_eq!(member_rows(&library, &duet).await, 0);
    }
    assert_eq!(library.rows("account_credit_splits").await, 0);
    assert_eq!(library.rows("artist_association_member_matches").await, 0);
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_listed_member_takes_the_identity_of_its_own_credit_and_the_rest_stay_reviewable() {
    let library = prepare("Ada & Bo", listed(&["Ada", "Bo"]), &[("Ada", &ADA_SONGS)]).await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    unknown(&library, "Ada & Bo");
    library.run().await;
    let ada = library.group_of(&recording_of(&library, 0)).await.unwrap();
    for duet in duets(&library) {
        assert_eq!(
            member_group(&library, &duet, "Ada").await.as_ref(),
            Some(&ada)
        );
        assert_eq!(member_group(&library, &duet, "Bo").await, None);
        assert_eq!(
            member_evidence(&library, &duet, "Ada").await,
            [ArtistAssociationEvidence::MatchedMemberSoloCredit]
        );
    }
    assert!(
        queued(&library).await.contains(&"Ada & Bo".to_owned()),
        "A credit with an unidentified member stays in the review queue"
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_member_choice_the_listener_reset_is_never_chosen_again() {
    let library = prepare("Ada & Bo", listed(&["Ada", "Bo"]), &[("Ada", &ADA_SONGS)]).await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    unknown(&library, "Ada & Bo");
    library.run().await;
    let duet = duets(&library)[0].clone();
    let ada = artist(&library, "Ada").await;
    let choice = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(&library.actor, &duet, &ada)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .reset_credit_member_identity(
            &library.actor,
            &duet,
            &ada,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    library.run().await;
    assert_eq!(member_group(&library, &duet, "Ada").await, None);
    assert!(
        member_group(&library, &duets(&library)[1], "Ada")
            .await
            .is_some()
    );
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_personally_edited_recording_gains_no_members() {
    let library = prepare(
        "Ada & Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    let duets = duets(&library);
    hito_core::test_support::support::synchronize(
        &library.hito,
        &library.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&duets[0]).unwrap(),
            artist: "Ada & Bo".into(),
        }],
    )
    .await;
    library.run().await;
    assert_eq!(member_rows(&library, &duets[0]).await, 0);
    assert!(library.associations(&duets[0]).await.is_empty());
    assert!(member_names(&library, &duets[0]).await.is_empty());
    for duet in &duets[1..] {
        assert_eq!(member_names(&library, duet).await, ["Ada", "Bo"]);
        assert!(member_group(&library, duet, "Ada").await.is_some());
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_split_whose_member_loses_its_own_identity_is_taken_away_on_the_next_walk() {
    let library = prepare(
        "Ada & Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    library.run().await;
    assert_eq!(member_rows(&library, &duets(&library)[0]).await, 2);
    for position in 0..2 {
        let recording = recording_of(&library, position);
        let choice = library
            .hito
            .artist_catalog
            .recording_artist_identity_choice(&library.actor, &recording)
            .await
            .unwrap();
        library
            .hito
            .artist_catalog
            .reset_recording_artist_identity(
                &library.actor,
                &recording,
                ResetRecordingArtistIdentity {
                    request_id: Uuid::new_v4(),
                    expected_revision: choice.revision,
                },
            )
            .await
            .unwrap();
    }
    library.run().await;
    for duet in duets(&library) {
        assert_eq!(member_rows(&library, &duet).await, 0);
    }
    assert_eq!(library.rows("account_credit_splits").await, 0);
    assert_eq!(
        library
            .scalar("SELECT count(*) FROM recording_artist_member_identity_choices WHERE retired=0")
            .await,
        "0"
    );
    assert!(queued(&library).await.contains(&"Ada & Bo".to_owned()));
    library.origin.shutdown().await;
}

#[tokio::test]
async fn erasing_an_account_removes_its_automatic_member_matches_and_splits() {
    let library = prepare(
        "Ada & Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    library.run().await;
    assert_eq!(library.rows("artist_association_member_matches").await, 6);
    assert_eq!(library.rows("account_credit_splits").await, 2);
    assert_eq!(library.rows("artist_name_misses").await, 1);
    library
        .hito
        .artist_catalog
        .erase_account_corrections(&library.actor, &library.actor.user().id)
        .await
        .unwrap();
    for table in [
        "artist_association_member_matches",
        "account_credit_splits",
        "artist_name_misses",
        "recording_artist_member_identity_choices",
    ] {
        assert_eq!(library.rows(table).await, 0, "{table}");
    }
    library.origin.shutdown().await;
}

async fn roles(library: &NameLibrary, recording: &str) -> Vec<(String, CreditRole)> {
    library
        .hito
        .catalog
        .recording_metadata(&library.actor, recording, MetadataHistoryQuery::default())
        .await
        .unwrap()
        .credited_artists
        .into_iter()
        .map(|member| (member.name, member.role))
        .collect()
}

#[tokio::test]
async fn a_featured_part_of_a_split_credit_stays_featured() {
    let library = prepare(
        "Ada feat. Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada feat. Bo");
    library.run().await;
    for duet in duets(&library) {
        assert_eq!(
            roles(&library, &duet).await,
            [
                ("Ada".to_owned(), CreditRole::Main),
                ("Bo".to_owned(), CreditRole::Featured)
            ]
        );
        assert!(member_group(&library, &duet, "Bo").await.is_some());
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_credit_whose_whole_identity_the_listener_cleared_is_never_split() {
    let library = prepare(
        "Ada & Bo",
        Evidence::default(),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    library
        .hito
        .artist_catalog
        .reset_recording_artist_identity(
            &library.actor,
            &duets(&library)[0],
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    library.run().await;
    assert_eq!(library.rows("artist_name_misses").await, 1);
    assert_eq!(library.rows("account_credit_splits").await, 0);
    for duet in duets(&library) {
        assert_eq!(member_rows(&library, &duet).await, 0);
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_member_name_with_accents_finds_its_own_credit() {
    let library = prepare(
        "Ólafur & Bo",
        Evidence::default(),
        &[("Ólafur", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ólafur", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ólafur & Bo");
    library.run().await;
    for duet in duets(&library) {
        assert_eq!(member_names(&library, &duet).await, ["Ólafur", "Bo"]);
        assert!(member_group(&library, &duet, "Ólafur").await.is_some());
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn a_short_member_name_is_found_among_many_longer_names_that_contain_it() {
    let library = prepare("Ada & Bo", listed(&["Ada", "Bo"]), &[("Bo", &BO_SONGS)]).await;
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    let library_id = library
        .scalar("SELECT library_id FROM account_artists WHERE name='Bo'")
        .await;
    library
        .execute_batch(&format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<250)
             INSERT INTO account_artists(user_id,id,library_id,name)
             SELECT (SELECT id FROM users WHERE username='admin'),'bobo-'||i,'{library_id}','Bobo '||i FROM n"
        ))
        .await;
    library.run().await;
    let bo = library.group_of(&recording_of(&library, 0)).await.unwrap();
    for duet in duets(&library) {
        assert_eq!(
            member_group(&library, &duet, "Bo").await.as_ref(),
            Some(&bo)
        );
    }
    library.origin.shutdown().await;
}

#[tokio::test]
async fn the_review_of_a_partly_identified_credit_counts_and_searches_only_its_unidentified_tracks()
{
    let library = prepare(
        "Ada & Bo",
        listed(&["Ada", "Bo"]),
        &[("Ada", &ADA_SONGS), ("Bo", &BO_SONGS)],
    )
    .await;
    known(&library, "Ada", ADA, &ADA_SONGS);
    known(&library, "Bo", BO, &BO_SONGS);
    unknown(&library, "Ada & Bo");
    library.run().await;
    assert!(!queued(&library).await.contains(&"Ada & Bo".to_owned()));
    let mut duets = duets(&library);
    duets.sort();
    let open = duets.last().unwrap().clone();
    let bo = artist(&library, "Bo").await;
    let choice = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(&library.actor, &open, &bo)
        .await
        .unwrap();
    library
        .hito
        .artist_catalog
        .reset_credit_member_identity(
            &library.actor,
            &open,
            &bo,
            ResetRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
            },
        )
        .await
        .unwrap();
    let item = library
        .hito
        .artist_catalog
        .artist_review_queue(&library.actor, ArtistReviewQueueQuery::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.name == "Ada & Bo")
        .unwrap();
    assert_eq!(item.undecided_tracks, 1);
    assert!(item.needs_search);
    assert_eq!(item.search_recording_id.as_ref(), Some(&open));
    library.origin.shutdown().await;
}
