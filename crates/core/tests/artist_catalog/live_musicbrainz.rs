use hito_core::test_support::support;
use hito_core::{
    Actor, AlbumEditionTrackListAvailability, ArtistDiscographyAvailability, Browse,
    EditionTrackListAvailability, MetadataProvider, ReadAlbumEditionTrackList,
    ReadArtistDiscography, ReadEditionTrackList, ReadReleaseGroupEditions,
    ReleaseGroupEditionsAvailability, Services,
};
use hito_plugin_sdk::{
    CatalogEditionTrackList, CatalogReleaseGroup, CatalogReleaseGroupKind, CatalogReleaseTypeFilter,
};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use uuid::Uuid;

const RADIOHEAD: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";
const DEADLINE: Duration = Duration::from_secs(90);

struct Chosen {
    hito: Services,
    actor: Actor,
    recording_id: String,
    release_id: String,
    association_id: String,
    _directory: tempfile::TempDir,
}

fn require_opt_in() {
    assert!(
        std::env::var("HITO_MUSICBRAINZ_LIVE").as_deref() == Ok("1"),
        "MusicBrainz live core check failed: stage=opt_in code=explicit_opt_in_required"
    );
}

async fn chosen_artist() -> Chosen {
    let (directory, hito, actor, library, music) = support::library("live-musicbrainz-core").await;
    let path = music.join("live.wav");
    support::write_wav(&path, 1);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Live acceptance track".into());
    tag.set_artist("Radiohead".into());
    tag.set_album("Live acceptance album".into());
    tag.save_to_path(&path, Default::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    let recording_id = hito
        .catalog
        .recordings(&actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let release_id = hito
        .catalog
        .releases(&actor, Browse::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let context = hito
        .artist_catalog
        .begin_artist_review(&actor, &recording_id, MetadataProvider::BuiltinMusicBrainz)
        .await
        .unwrap();
    let association = hito
        .artist_catalog
        .confirm_artist_review(&actor, &context, RADIOHEAD)
        .await
        .unwrap();
    Chosen {
        hito,
        actor,
        recording_id,
        release_id,
        association_id: association.id,
        _directory: directory,
    }
}

fn canonical_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}

impl Chosen {
    async fn discography(
        &self,
        release_type: Option<CatalogReleaseTypeFilter>,
        limit: u16,
        continuation: Option<String>,
    ) -> (Vec<CatalogReleaseGroup>, Option<String>, Option<u32>) {
        let ArtistDiscographyAvailability::Available {
            association_id,
            page,
            continuation,
        } = self
            .hito
            .artist_catalog
            .recording_artist_discography_until(
                &self.actor,
                &self.recording_id,
                &self.association_id,
                ReadArtistDiscography {
                    limit,
                    continuation,
                    release_type,
                },
                std::future::pending(),
            )
            .await
            .expect("discography read failed: code=core_error")
        else {
            panic!("discography read failed: code=provider_unavailable");
        };
        assert_eq!(association_id, self.association_id);
        assert_eq!(page.artist_id, RADIOHEAD);
        (page.items, continuation, page.total_count)
    }

    async fn editions(&self, release_group_id: &str, limit: u16) -> Vec<String> {
        let ReleaseGroupEditionsAvailability::Available { page, .. } = self
            .hito
            .artist_catalog
            .recording_artist_release_group_editions_until(
                &self.actor,
                &self.recording_id,
                &self.association_id,
                ReadReleaseGroupEditions {
                    release_group_id: release_group_id.into(),
                    limit,
                    continuation: None,
                },
                std::future::pending(),
            )
            .await
            .expect("edition listing failed: code=core_error")
        else {
            panic!("edition listing failed: code=provider_unavailable");
        };
        assert_eq!(page.release_group_id, release_group_id);
        page.items.into_iter().map(|edition| edition.id).collect()
    }

    async fn artist_edition_tracks(
        &self,
        release_group_id: &str,
        edition_id: &str,
    ) -> CatalogEditionTrackList {
        let EditionTrackListAvailability::Available { edition, .. } = self
            .hito
            .artist_catalog
            .recording_artist_edition_track_list_until(
                &self.actor,
                &self.recording_id,
                &self.association_id,
                ReadEditionTrackList {
                    release_group_id: release_group_id.into(),
                    edition_id: edition_id.into(),
                },
                std::future::pending(),
            )
            .await
            .expect("edition tracks failed: code=core_error")
        else {
            panic!("edition tracks failed: code=provider_unavailable");
        };
        edition
    }
}

fn assert_disc_and_track_order(edition: &CatalogEditionTrackList) -> usize {
    assert!(!edition.media.is_empty(), "no media");
    let mut previous_medium = 0;
    let mut tracks = 0;
    for medium in &edition.media {
        assert!(medium.position > previous_medium, "media out of order");
        previous_medium = medium.position;
        let mut previous_track = 0;
        for track in &medium.tracks {
            assert!(track.position > previous_track, "tracks out of order");
            previous_track = track.position;
            assert!(!track.title.is_empty());
            if let Some(recording) = &track.recording_id {
                assert!(canonical_id(recording), "recording identity not canonical");
            }
            tracks += 1;
        }
    }
    assert!(tracks > 0, "no tracks");
    tracks
}

#[tokio::test]
#[ignore = "Requires HITO_MUSICBRAINZ_LIVE=1 and the public MusicBrainz service"]
async fn core_pages_a_real_artists_discography_and_distinct_editions_through_the_built_in_provider()
{
    require_opt_in();
    let started = Instant::now();
    tokio::time::timeout(DEADLINE, async {
        let chosen = chosen_artist().await;
        let (first, continuation, total) = chosen.discography(None, 10, None).await;
        assert!(!first.is_empty() && first.len() <= 10);
        assert!(total.is_some_and(|total| total as usize >= first.len()));
        let mut seen = HashSet::new();
        let mut all = first;
        if let Some(token) = continuation {
            let (second, _, _) = chosen.discography(None, 10, Some(token)).await;
            all.extend(second);
        }
        for group in &all {
            assert!(canonical_id(&group.id), "release group identity");
            assert!(seen.insert(group.id.clone()), "repeated release group");
            assert!(!group.title.is_empty());
        }
        assert!(
            all.iter()
                .any(|group| group.kind == Some(CatalogReleaseGroupKind::Album)),
            "no album among the paged groups"
        );

        let (albums, _, _) = chosen
            .discography(Some(CatalogReleaseTypeFilter::Album), 5, None)
            .await;
        assert!(!albums.is_empty());
        assert!(
            albums
                .iter()
                .all(|group| group.kind == Some(CatalogReleaseGroupKind::Album))
        );

        let editions = chosen.editions(&albums[0].id, 10).await;
        let distinct: HashSet<_> = editions.iter().collect();
        assert!(editions.len() >= 2, "expected several editions");
        assert_eq!(distinct.len(), editions.len(), "repeated edition");
        assert!(editions.iter().all(|id| canonical_id(id)));
        tracing::info!(
            workflow = "musicbrainz_live_core_discography",
            outcome = "success",
            paged_groups = all.len(),
            editions = editions.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz live core discography check completed"
        );
    })
    .await
    .expect("MusicBrainz live core check failed: code=deadline_exceeded");
}

#[tokio::test]
#[ignore = "Requires HITO_MUSICBRAINZ_LIVE=1 and the public MusicBrainz service"]
async fn core_reads_a_real_release_tracklist_in_disc_and_track_order_for_an_album_page() {
    require_opt_in();
    let started = Instant::now();
    tokio::time::timeout(DEADLINE, async {
        let chosen = chosen_artist().await;
        let (albums, _, _) = chosen
            .discography(Some(CatalogReleaseTypeFilter::Album), 5, None)
            .await;
        let group = albums
            .iter()
            .find(|group| group.secondary_types.is_empty())
            .unwrap_or(&albums[0]);
        let editions = chosen.editions(&group.id, 5).await;
        let edition_id = editions.first().expect("no edition").clone();

        let listed = chosen.artist_edition_tracks(&group.id, &edition_id).await;
        let tracks = assert_disc_and_track_order(&listed);
        assert_eq!(listed.id, edition_id);
        assert_eq!(listed.release_group_id, group.id);

        let AlbumEditionTrackListAvailability::Available {
            edition,
            matches,
            library_only,
            ..
        } = chosen
            .hito
            .artist_catalog
            .album_edition_track_list_until(
                &chosen.actor,
                &chosen.release_id,
                ReadAlbumEditionTrackList {
                    recording_id: chosen.recording_id.clone(),
                    association_id: chosen.association_id.clone(),
                    release_group_id: group.id.clone(),
                    edition_id: edition_id.clone(),
                },
                std::future::pending(),
            )
            .await
            .expect("album track list failed: code=core_error")
        else {
            panic!("album track list failed: code=provider_unavailable");
        };
        assert_eq!(assert_disc_and_track_order(&edition), tracks);
        assert!(*edition == listed, "album read differs from artist read");
        assert!(
            matches.is_empty(),
            "an unlinked library track must not match"
        );
        assert_eq!(library_only.len(), 1);
        tracing::info!(
            workflow = "musicbrainz_live_core_album_tracklist",
            outcome = "success",
            media = edition.media.len(),
            tracks,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz live core track list check completed"
        );
    })
    .await
    .expect("MusicBrainz live core check failed: code=deadline_exceeded");
}
