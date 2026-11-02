#[path = "../fixtures/credited_artist_library.rs"]
#[allow(dead_code)]
mod library;

use hito_core::test_support::collection_artwork::{ArtistKind, ArtworkServer};
use hito_core::{Actor, ArtistIdentityGroupQuery, ImportProviderPlaylist, InstallPlugin};
use library::{Credited, Group, MUSICBRAINZ_ARTIST, imported, imported_two_credits};
use uuid::Uuid;

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

const STRANGER: &str = "(SELECT id FROM users WHERE username = 'stranger')";

async fn stranger_with_colliding_plugin_credit(t: &Credited) -> (Actor, String) {
    let stranger = t.another_account().await;
    let installed = t
        .hito
        .plugins
        .install(
            &stranger,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: t.text("SELECT id FROM plugin_services").await,
                service_revision: t.scalar("SELECT revision FROM plugin_services").await,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    t.hito
        .collection
        .import_provider_playlist(
            &stranger,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/owned-artwork".into(),
            },
        )
        .await
        .unwrap();
    let artist = t
        .text(&format!(
            "SELECT id FROM account_artists WHERE user_id = {STRANGER} LIMIT 1"
        ))
        .await;
    let recording = t
        .text(&format!(
            "SELECT r.id FROM recordings r JOIN libraries l ON l.id = r.library_id WHERE l.owner_id = {STRANGER} LIMIT 1"
        ))
        .await;
    t.run_unchecked(&[
        format!("INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,plugin_installation_id,provider_artist_id) VALUES('group-plugin-stranger',{STRANGER},'http_plugin','{installation}','plugin-artist')", installation = installed.id),
        format!("INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,observation_id,role,provider_kind,plugin_installation_id,provider_artist_id,evidence) VALUES('association-stranger',{STRANGER},(SELECT library_id FROM recordings WHERE id='{recording}'),'{recording}','{artist}',(SELECT metadata_observation_id FROM recordings WHERE id='{recording}'),'recording_artist','http_plugin','{installation}','plugin-artist','user_confirmed')", installation = installed.id),
        format!("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) VALUES({STRANGER},'{recording}',1,'association-stranger','group-plugin-stranger','{artist}',(SELECT metadata_observation_id FROM recordings WHERE id='{recording}'),1)"),
    ])
    .await;
    (stranger, artist)
}

async fn photo_of(t: &Credited, actor: &Actor, group: &str) -> Option<String> {
    let listed = t
        .hito
        .artist_catalog
        .artist_identity_groups(actor, ArtistIdentityGroupQuery::default())
        .await
        .unwrap();
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].id, group);
    let detail = t
        .hito
        .artist_catalog
        .artist_identity_group(actor, group, Default::default())
        .await
        .unwrap();
    assert_eq!(
        detail.group.credited_photo_artist_id,
        listed.items[0].credited_photo_artist_id
    );
    listed.items[0].credited_photo_artist_id.clone()
}

#[tokio::test]
async fn two_accounts_with_colliding_plugin_credits_each_get_only_their_own_artist() {
    let t = served().await;
    choose_the_only_recording(&t, Group::Plugin).await;
    let (stranger, stranger_artist) = stranger_with_colliding_plugin_credit(&t).await;
    assert_ne!(t.artist, stranger_artist);
    assert_eq!(
        photo_of(&t, &t.actor, Group::Plugin.id()).await,
        Some(t.artist.clone())
    );
    assert_eq!(
        photo_of(&t, &stranger, "group-plugin-stranger").await,
        Some(stranger_artist)
    );
    for (actor, foreign) in [
        (&t.actor, "group-plugin-stranger"),
        (&stranger, Group::Plugin.id()),
    ] {
        assert!(
            t.hito
                .artist_catalog
                .artist_identity_group(actor, foreign, Default::default())
                .await
                .is_err()
        );
    }
}

async fn group_page(t: &Credited, offset: i64) -> (i64, Vec<hito_core::ArtistIdentityGroup>) {
    let page = t
        .hito
        .artist_catalog
        .artist_identity_groups(
            &t.actor,
            ArtistIdentityGroupQuery {
                offset: Some(offset),
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    (page.total, page.items)
}

#[tokio::test]
async fn each_group_of_a_page_gets_its_own_photo_and_the_next_page_gets_its_own() {
    let t = five_recordings().await;
    let first = t.credit("Observed performer").await;
    let second = t.credit("Second performer").await;
    let r = recordings(&t).await;
    let expected = [
        ("group-a", &first, &r[0]),
        ("group-b", &second, &r[3]),
        ("group-c", &first, &r[2]),
    ];
    for (order, (group, credit, recording)) in expected.iter().enumerate() {
        t.choose(Group::Other(group), recording, credit, order as i64 + 1)
            .await;
    }
    let (total, first_page) = group_page(&t, 0).await;
    let (_, second_page) = group_page(&t, 2).await;
    assert_eq!(total, 3);
    assert_eq!((first_page.len(), second_page.len()), (2, 1));
    let mut seen = Vec::new();
    for item in first_page.iter().chain(&second_page) {
        let (_, credit, _) = expected
            .iter()
            .find(|(group, _, _)| *group == item.id)
            .unwrap();
        assert_eq!(
            item.credited_photo_artist_id.as_ref(),
            Some(*credit),
            "{}",
            item.id
        );
        seen.push(item.id.clone());
    }
    seen.sort();
    assert_eq!(seen, ["group-a", "group-b", "group-c"]);
}
