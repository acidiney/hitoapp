#[path = "../fixtures/credited_artist_library.rs"]
#[allow(dead_code)]
mod library;

use hito_core::test_support::collection_artwork::{
    ARTIST_ID, ARTIST_PHOTO, ArtistKind, ArtworkServer,
};
use hito_core::{CoverArtKind, CoverArtLookup};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use library::{MUSICBRAINZ_ARTIST, imported};

fn artist_request() -> ArtworkRequest {
    ArtworkRequest {
        entity: ArtworkEntity::Artist,
        provider_id: ARTIST_ID.into(),
    }
}

#[tokio::test]
async fn a_credit_known_only_through_a_provider_gets_its_photo_from_the_plugin_once() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    assert!(t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert_eq!(t.drive().await, 1);
    let CoverArtLookup::Ready(image) = t.photo().await else {
        panic!("the photo must be stored");
    };
    assert_eq!(image.bytes, ARTIST_PHOTO);
    assert_eq!(t.drive().await, 0);
    assert_eq!(t.provider.requests().await, [artist_request()]);
}

#[tokio::test]
async fn a_credit_without_a_provider_artist_id_is_never_queued_or_requested() {
    let t = imported(ArtworkServer::start().await).await;
    assert!(!t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
}

#[tokio::test]
async fn with_automatic_artwork_off_nothing_is_queued_requested_or_served() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    t.automatic_artwork(false).await;
    assert!(!t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
    t.automatic_artwork(true).await;
    assert!(t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    t.drive().await;
    assert!(matches!(t.photo().await, CoverArtLookup::Ready(_)));
    t.automatic_artwork(false).await;
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
}

#[tokio::test]
async fn a_plugin_that_does_not_know_the_artist_kind_leaves_the_credit_without_a_photo() {
    for refusal in [ArtistKind::Refused, ArtistKind::Unprocessable] {
        let t = imported(ArtworkServer::start_with_credited_artist(refusal).await).await;
        assert!(matches!(t.photo().await, CoverArtLookup::Pending));
        assert_eq!(t.drive().await, 1);
        assert!(matches!(t.photo().await, CoverArtLookup::Missing));
        assert_eq!(t.drive().await, 0);
        assert_eq!(t.provider.requests().await, [artist_request()]);
    }
}

#[tokio::test]
async fn an_artist_identifier_that_is_not_an_owned_credit_is_refused() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    for artist in ["", "not an id", &"a".repeat(65)] {
        assert!(
            t.hito
                .catalog
                .cover_art(&t.actor, CoverArtKind::CreditedArtist, artist)
                .await
                .is_err()
        );
    }
    let stranger = "f".repeat(32);
    assert!(matches!(
        t.hito
            .catalog
            .cover_art(&t.actor, CoverArtKind::CreditedArtist, &stranger)
            .await
            .unwrap(),
        CoverArtLookup::Unknown
    ));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
}

#[tokio::test]
async fn a_chosen_musicbrainz_identity_keeps_the_photo_until_wikidata_has_none() {
    let t = imported(ArtworkServer::start_with_credited_artist(ArtistKind::Served).await).await;
    t.choose_musicbrainz_identity().await;
    assert!(!t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Unknown));
    assert_eq!(t.drive().await, 0);
    assert!(t.provider.requests().await.is_empty());
    let account = "(SELECT id FROM users)";
    t.run_unchecked(&[format!(
        "INSERT INTO cover_art_lookups(user_id,kind,mbid,state) VALUES({account},'artist','{MUSICBRAINZ_ARTIST}','missing')"
    )])
    .await;
    assert!(t.provider_photo_offered().await);
    assert!(matches!(t.photo().await, CoverArtLookup::Pending));
    assert_eq!(t.drive().await, 1);
    assert!(matches!(t.photo().await, CoverArtLookup::Ready(_)));
    assert_eq!(t.provider.requests().await, [artist_request()]);
}
