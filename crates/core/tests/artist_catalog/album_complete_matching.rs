#[path = "../fixtures/album_complete_library.rs"]
#[allow(dead_code)]
mod library;
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::test_support::support;

use hito_core::{Action, AlbumEditionTrackListAvailability, Browse, UpdateMetadataSettings};
use library::Album;
use uuid::Uuid;

const MAIN_SECOND: &str = "occurrence:album/1/2";
const BONUS_DISC: &str = "occurrence:album/2/2";

struct Merged {
    linked: Vec<String>,
    attached: Vec<String>,
    library_only: Vec<String>,
}

async fn merged(album: &Album) -> Merged {
    let AlbumEditionTrackListAvailability::Available {
        matches,
        attached_occurrences,
        library_only,
        ..
    } = album.read(provider::EDITION_ID).await.unwrap()
    else {
        panic!("The edition must be available");
    };
    Merged {
        linked: matches.into_iter().map(|m| m.occurrence_id).collect(),
        attached: attached_occurrences,
        library_only: library_only.into_iter().map(|t| t.recording.id).collect(),
    }
}

async fn anchor(album: &Album, offset: Option<i64>) -> Option<hito_core::AlbumCatalogAnchor> {
    album
        .hito
        .catalog
        .release(
            &album.actor,
            &album.release_id,
            Browse {
                offset,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .catalog_anchor
}

async fn settings(
    album: &Album,
    enabled: bool,
    providers: Option<Vec<hito_core::MetadataProvider>>,
) {
    let current = album
        .hito
        .catalog
        .metadata_settings(&album.actor)
        .await
        .unwrap();
    album
        .hito
        .catalog
        .set_metadata_settings(
            &album.actor,
            UpdateMetadataSettings {
                revision: current.revision,
                enabled,
                providers: providers.unwrap_or(current.providers),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_retired_binding_no_longer_links_its_recording() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    assert_eq!(merged(&album).await.linked.len(), 3);
    album
        .sql(
            "UPDATE provider_items SET retired = 1, revision = revision + 1 WHERE recording_id = ? AND role = 'primary'",
            &[&album.second],
        )
        .await;
    let result = merged(&album).await;
    assert!(!result.linked.contains(&MAIN_SECOND.to_owned()));
    assert_eq!(result.linked.len(), 2);
    assert!(result.library_only.contains(&album.second));
    assert!(result.attached.is_empty());
    assert!(server.finish().await);
}

#[tokio::test]
async fn another_installation_reusing_a_provider_id_never_links() {
    let mut server = AlbumProvider::start().await;
    let mut other = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let second_installation = library::connect(&album.hito, &album.actor, &other).await;
    let foreign = library::adopt(
        &album.hito,
        &album.actor,
        &second_installation,
        provider::BONUS,
    )
    .await;
    let result = merged(&album).await;
    assert!(!result.linked.contains(&BONUS_DISC.to_owned()));
    assert!(result.attached.is_empty());
    assert!(!result.library_only.contains(&foreign.id));
    assert_eq!(result.linked.len(), 3);
    assert!(server.finish().await);
    assert!(other.finish().await);
}

#[tokio::test]
async fn a_source_only_binding_stays_catalog_only_and_is_reported_as_attached_elsewhere() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    album
        .sql(
            "INSERT INTO provider_items(id, installation_id, provider_recording_id, recording_id, role, evidence)
             VALUES (?, ?, ?, ?, 'source', 'user_confirmed')",
            &[
                &Uuid::new_v4().to_string(),
                &album.installation.id,
                provider::BONUS,
                &album.extra,
            ],
        )
        .await;
    let result = merged(&album).await;
    assert!(!result.linked.contains(&BONUS_DISC.to_owned()));
    assert_eq!(result.attached, [BONUS_DISC]);
    assert_eq!(result.linked.len(), 3);
    let saved = library::adopt(
        &album.hito,
        &album.actor,
        &album.installation,
        provider::BONUS,
    )
    .await;
    assert_eq!(
        saved.id, album.extra,
        "Saving an attached provider id returns the other recording, so Play must not be offered"
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn a_primary_binding_that_is_not_the_current_metadata_binding_does_not_link() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let replacement = Uuid::new_v4().to_string();
    album
        .sql(
            "INSERT INTO provider_items(id, installation_id, provider_recording_id, recording_id, role, evidence)
             VALUES (?, ?, 'recording:album/replacement', ?, 'primary', 'provider_identity')",
            &[&replacement, &album.installation.id, &album.second],
        )
        .await;
    album
        .sql(
            "UPDATE recordings SET metadata_binding_id = ? WHERE id = ?",
            &[&replacement, &album.second],
        )
        .await;
    let result = merged(&album).await;
    assert!(!result.linked.contains(&MAIN_SECOND.to_owned()));
    assert_eq!(result.attached, [MAIN_SECOND]);
    assert_eq!(result.linked.len(), 2);
    assert!(server.finish().await);
}

#[tokio::test]
async fn the_anchor_exists_only_while_its_reviewed_provider_can_be_read() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    assert_eq!(
        anchor(&album, None).await.unwrap().recording_id,
        album.first
    );
    assert!(
        anchor(&album, Some(1)).await.is_none(),
        "Only the first page computes the anchor"
    );

    settings(&album, false, None).await;
    assert!(
        anchor(&album, None).await.is_none(),
        "Disabled providers hide it"
    );
    settings(
        &album,
        true,
        Some(vec![hito_core::MetadataProvider::BuiltinMusicBrainz]),
    )
    .await;
    assert!(
        anchor(&album, None).await.is_none(),
        "An unselected provider hides it"
    );
    let provider = hito_core::MetadataProvider::HttpPlugin {
        installation_id: album.installation.id.clone(),
    };
    settings(&album, true, Some(vec![provider])).await;
    assert!(anchor(&album, None).await.is_some());

    support::synchronize(
        &album.hito,
        &album.actor,
        vec![Action::RecordingArtistSet {
            recording_id: Uuid::parse_str(&album.first).unwrap(),
            artist: "Someone else entirely".into(),
        }],
    )
    .await;
    assert!(
        anchor(&album, None).await.is_none(),
        "An association whose credit changed would fail on use, so it is not offered"
    );
    assert!(server.finish().await);
}

#[tokio::test]
async fn the_anchor_prefers_the_album_artist_over_a_featured_credit() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare_with_featured(&server).await;
    let featured = album.featured.clone().unwrap();
    let detail = album
        .hito
        .catalog
        .release(&album.actor, &album.release_id, Browse::default())
        .await
        .unwrap();
    assert_eq!(detail.tracks.items[0].recording.id, featured);
    let anchor = detail.catalog_anchor.unwrap();
    assert_eq!(anchor.recording_id, album.first);
    assert_ne!(anchor.recording_id, featured);
    assert!(server.finish().await);
}
