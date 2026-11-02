#[path = "../fixtures/credited_artist_library.rs"]
#[allow(dead_code)]
mod library;

use hito_core::ArtistIdentityGroupQuery;
use hito_core::test_support::collection_artwork::{ArtistKind, ArtworkServer};
use library::{Credited, Group, MUSICBRAINZ_ARTIST, imported, imported_two_credits};

async fn served() -> Credited {
    imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await
}

async fn five_recordings() -> Credited {
    imported_two_credits(
        ArtworkServer::start_with_credited_artist(ArtistKind::TwoCreditsOnFiveRecordings).await,
    )
    .await
}

async fn musicbrainz_photo(t: &Credited, state: &str) {
    t.run_unchecked(&[format!(
        "INSERT INTO cover_art_lookups(user_id,kind,mbid,state,content_hash) VALUES((SELECT id FROM users),'artist','{MUSICBRAINZ_ARTIST}','{state}',{hash})",
        hash = if state == "ready" { "'0000'" } else { "NULL" }
    )])
    .await;
}

async fn choose_the_only_recording(t: &Credited, group: Group) {
    let recording = t.recording("Observed recording").await;
    t.choose(group, &recording, &t.artist, 1).await;
}

#[tokio::test]
async fn a_group_without_a_musicbrainz_identity_gets_the_photo_of_a_qualifying_member() {
    let t = served().await;
    choose_the_only_recording(&t, Group::Plugin).await;
    assert_eq!(t.group_photo(Group::Plugin).await, Some(t.artist.clone()));
}

#[tokio::test]
async fn a_group_whose_musicbrainz_photo_is_missing_gets_the_photo_of_a_qualifying_member() {
    let t = served().await;
    choose_the_only_recording(&t, Group::MusicBrainz).await;
    musicbrainz_photo(&t, "missing").await;
    assert_eq!(
        t.group_photo(Group::MusicBrainz).await,
        Some(t.artist.clone())
    );
}

#[tokio::test]
async fn a_musicbrainz_photo_that_is_present_or_still_pending_keeps_the_musicbrainz_path() {
    for state in ["ready", "queued", "running", "unavailable"] {
        let t = served().await;
        choose_the_only_recording(&t, Group::MusicBrainz).await;
        musicbrainz_photo(&t, state).await;
        assert_eq!(t.group_photo(Group::MusicBrainz).await, None, "{state}");
    }
    let t = served().await;
    choose_the_only_recording(&t, Group::MusicBrainz).await;
    assert_eq!(t.group_photo(Group::MusicBrainz).await, None, "unlooked");
}

#[tokio::test]
async fn a_group_without_a_qualifying_member_gets_no_plugin_photo() {
    let t = imported(ArtworkServer::start().await).await;
    choose_the_only_recording(&t, Group::Plugin).await;
    assert_eq!(t.group_photo(Group::Plugin).await, None);
}

#[tokio::test]
async fn the_cheap_gate_hides_the_photo_when_automatic_artwork_is_off_or_the_connection_is_inactive()
 {
    let t = served().await;
    choose_the_only_recording(&t, Group::Plugin).await;
    t.automatic_artwork(false).await;
    assert_eq!(t.group_photo(Group::Plugin).await, None);
    t.automatic_artwork(true).await;
    assert_eq!(t.group_photo(Group::Plugin).await, Some(t.artist.clone()));
    t.run_unchecked(&["UPDATE plugin_installations SET active = 0".into()])
        .await;
    assert_eq!(t.group_photo(Group::Plugin).await, None);
}

#[tokio::test]
async fn another_accounts_view_never_shows_the_members_or_their_photo() {
    let t = served().await;
    choose_the_only_recording(&t, Group::Plugin).await;
    let stranger = t.another_account().await;
    let page = t
        .hito
        .artist_catalog
        .artist_identity_groups(&stranger, ArtistIdentityGroupQuery::default())
        .await
        .unwrap();
    assert!(page.items.is_empty());
    assert!(
        t.hito
            .artist_catalog
            .artist_identity_group(&stranger, Group::Plugin.id(), Default::default())
            .await
            .is_err()
    );
}

async fn recordings(t: &Credited) -> [String; 5] {
    [
        t.recording("Observed recording 0").await,
        t.recording("Observed recording 1").await,
        t.recording("Observed recording 2").await,
        t.recording("Observed recording 3").await,
        t.recording("Observed recording 4").await,
    ]
}

#[tokio::test]
async fn the_member_with_the_most_recordings_in_the_group_wins_whatever_its_identifier() {
    for first_wins in [true, false] {
        let t = five_recordings().await;
        let first = t.credit("Observed performer").await;
        let second = t.credit("Second performer").await;
        let r = recordings(&t).await;
        if first_wins {
            t.choose(Group::Plugin, &r[0], &first, 1).await;
            t.choose(Group::Plugin, &r[1], &first, 2).await;
            t.choose(Group::Plugin, &r[3], &second, 3).await;
        } else {
            t.choose(Group::Plugin, &r[0], &first, 1).await;
            t.choose(Group::Plugin, &r[3], &second, 2).await;
            t.choose(Group::Plugin, &r[4], &second, 3).await;
        }
        let expected = if first_wins { first } else { second };
        assert_eq!(t.group_photo(Group::Plugin).await, Some(expected));
    }
}

#[tokio::test]
async fn equal_counts_are_broken_by_the_smaller_identifier() {
    let t = five_recordings().await;
    let first = t.credit("Observed performer").await;
    let second = t.credit("Second performer").await;
    let r = recordings(&t).await;
    t.choose(Group::Plugin, &r[0], &first, 1).await;
    t.choose(Group::Plugin, &r[3], &second, 2).await;
    assert_eq!(t.group_photo(Group::Plugin).await, Some(first.min(second)));
}

#[tokio::test]
async fn a_member_credit_that_a_present_musicbrainz_photo_already_covers_is_skipped() {
    let t = five_recordings().await;
    let first = t.credit("Observed performer").await;
    let second = t.credit("Second performer").await;
    let r = recordings(&t).await;
    t.choose(Group::Plugin, &r[0], &first, 1).await;
    t.choose(Group::Plugin, &r[1], &first, 2).await;
    t.choose(Group::Plugin, &r[3], &second, 3).await;
    t.choose(Group::MusicBrainz, &r[2], &first, 4).await;
    musicbrainz_photo(&t, "ready").await;
    assert_eq!(t.group_photo(Group::Plugin).await, Some(second));
}
