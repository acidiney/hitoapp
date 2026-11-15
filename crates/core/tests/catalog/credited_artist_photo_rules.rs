#[path = "../fixtures/credited_artist_library.rs"]
#[allow(dead_code)]
mod library;

use hito_core::test_support::collection_artwork::{ArtistKind, ArtworkServer};
use hito_core::{CoverArtKind, CoverArtLookup};
use hito_plugin_sdk::ArtworkEntity;
use library::{Credited, MUSICBRAINZ_ARTIST, imported};

async fn served() -> Credited {
    imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await
}

async fn artist_requests(t: &Credited) -> usize {
    t.provider
        .requests()
        .await
        .iter()
        .filter(|request| request.entity == ArtworkEntity::Artist)
        .count()
}

#[tokio::test]
async fn credited_photos_have_their_own_cap_apart_from_musicbrainz_photos() {
    let t = served().await;
    t.queue_unrelated_lookups("artist", 32).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));

    let t = served().await;
    t.queue_unrelated_lookups("credited_artist", 32).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    let musicbrainz = t
        .hito
        .catalog
        .cover_art(&t.actor, CoverArtKind::Artist, MUSICBRAINZ_ARTIST)
        .await
        .unwrap();
    assert!(matches!(musicbrainz, CoverArtLookup::Pending));
}

#[tokio::test]
async fn credited_photos_stay_inside_the_total_of_sixty_four_pending_lookups() {
    let t = served().await;
    t.queue_unrelated_lookups("release", 63).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));

    let t = served().await;
    t.queue_unrelated_lookups("release", 64).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
}

#[tokio::test]
async fn a_busy_plugin_transport_requeues_the_lookup_without_counting_an_attempt() {
    let t = served().await;
    let recording = t
        .hito
        .catalog
        .recordings(&t.actor, Default::default())
        .await
        .unwrap()
        .items[0]
        .id
        .clone();
    t.provider.hold_recording_requests();
    t.hito
        .catalog
        .enqueue_recording_artwork(&t.actor, &recording)
        .await
        .unwrap();
    let catalog = t.hito.catalog.clone();
    let running = tokio::spawn(async move {
        catalog
            .run_next_artwork_job_until(std::future::pending::<()>())
            .await
            .unwrap()
    });
    t.provider.recording_request_entered().await;

    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    for _ in 0..5 {
        assert_eq!(t.drive().await, 1);
        t.run_unchecked(&["UPDATE cover_art_lookups SET next_attempt_at = 0".into()])
            .await;
    }
    assert_eq!(artist_requests(&t).await, 0);
    assert_eq!(
        t.scalar("SELECT attempts FROM cover_art_lookups WHERE kind = 'credited_artist'")
            .await,
        0
    );
    assert_eq!(
        t.scalar("SELECT state = 'queued' FROM cover_art_lookups WHERE kind = 'credited_artist'")
            .await,
        1
    );

    t.provider.release_recording_requests();
    assert!(running.await.unwrap());
    assert_eq!(t.drive().await, 1);
    assert!(matches!(t.photo().await, CoverArtLookup::Ready(_)));
    assert_eq!(artist_requests(&t).await, 1);
}

async fn ready_photo() -> Credited {
    let t = served().await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    t.drive().await;
    assert!(matches!(t.photo().await, CoverArtLookup::Ready(_)));
    t
}

#[tokio::test]
async fn a_stored_photo_is_not_served_once_the_connection_is_deactivated() {
    let t = ready_photo().await;
    t.run_unchecked(&["UPDATE plugin_installations SET active = 0".into()])
        .await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
}

#[tokio::test]
async fn a_stored_photo_is_not_served_once_artwork_is_revoked() {
    let t = ready_photo().await;
    t.run_unchecked(&[
        "UPDATE plugin_installations SET manifest = json_set(manifest,'$.capabilities',json('[\"playlists\"]'))"
            .into(),
    ])
    .await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
}

#[tokio::test]
async fn a_stored_photo_is_not_served_once_the_recording_is_removed() {
    let t = ready_photo().await;
    t.run_unchecked(&["DELETE FROM account_recording_credits".into()])
        .await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
}

#[tokio::test]
async fn another_account_can_neither_queue_nor_read_a_credited_photo() {
    let t = ready_photo().await;
    let stranger = t.another_account().await;
    let lookups = t.scalar("SELECT count(*) FROM cover_art_lookups").await;
    let seen = t
        .hito
        .catalog
        .cover_art(&stranger, CoverArtKind::CreditedArtist, &t.artist)
        .await
        .unwrap();
    assert!(matches!(seen, CoverArtLookup::Unknown));
    assert_eq!(
        t.scalar("SELECT count(*) FROM cover_art_lookups").await,
        lookups
    );
    assert_eq!(t.drive().await, 0);
    assert_eq!(artist_requests(&t).await, 1);
}

#[tokio::test]
async fn turning_automatic_artwork_off_after_queueing_sends_no_plugin_request() {
    let t = served().await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    t.automatic_artwork(false).await;
    t.drive().await;
    assert_eq!(artist_requests(&t).await, 0);
    assert_eq!(
        t.scalar("SELECT count(*) FROM cover_art_lookups WHERE kind = 'credited_artist'")
            .await,
        0
    );
}

#[tokio::test]
async fn revoking_artwork_after_queueing_sends_no_plugin_request() {
    let t = served().await;
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    t.run_unchecked(&[
        "UPDATE plugin_installations SET manifest = json_set(manifest,'$.capabilities',json('[\"playlists\"]'))"
            .into(),
    ])
    .await;
    assert_eq!(t.drive().await, 1);
    assert_eq!(artist_requests(&t).await, 0);
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(
        t.scalar(&format!(
            "SELECT count(*) FROM cover_art_lookups WHERE user_id = '{}' AND kind = 'credited_artist' AND mbid = '{}'",
            t.actor.user().id,
            t.artist
        ))
        .await,
        0
    );
    assert_eq!(t.drive().await, 0);
    assert_eq!(artist_requests(&t).await, 0);
}

#[tokio::test]
async fn a_musicbrainz_photo_that_is_queued_or_unavailable_keeps_the_musicbrainz_path() {
    for state in ["queued", "unavailable", "running"] {
        let t = served().await;
        t.choose_musicbrainz_identity().await;
        t.run_unchecked(&[format!(
            "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES((SELECT id FROM users),'artist','{MUSICBRAINZ_ARTIST}','{state}')"
        )])
        .await;
        assert!(!t.provider_photo_offered().await, "{state}");
        assert!(
            matches!(t.photo().await, CoverArtLookup::Unknown),
            "{state}"
        );
        assert_eq!(artist_requests(&t).await, 0);
    }
}
