use crate::test_support::album_edition_service::{self as provider, AlbumProvider};
use crate::test_support::{support, work_meter::WorkMeter};
use crate::{
    ArtistCreditRole, AssociateRecordingArtist, Browse, InstallPlugin, MetadataProvider,
    OfferPluginService, Search, Services, UpdateMetadataSettings,
};
use lofty::{
    config::WriteOptions,
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};

const PAGE: i64 = 20;
const FIXED_WORK: u64 = 20_000;
const ANCHOR_WORK_PER_TRACK: u64 = 400;

fn write_album(music: &std::path::Path, album: &str, tracks: usize, base: i16) {
    let folder = music.join(album);
    std::fs::create_dir(&folder).unwrap();
    for track in 0..tracks {
        let path = folder.join(format!("track-{track:03}.wav"));
        support::write_wav(&path, base + track as i16);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("{album} track {track}"));
        tag.set_artist("Anchor work performer".into());
        tag.set_album(album.into());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
}

async fn release_id(hito: &Services, actor: &crate::Actor, title: &str) -> String {
    hito.catalog
        .releases(actor, Browse::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|release| release.title == title)
        .unwrap()
        .id
}

#[tokio::test]
async fn the_album_anchor_costs_work_proportional_to_the_first_page_album_only() {
    let (_directory, hito, actor, library, music) = support::library("anchor-work").await;
    write_album(&music, "Anchor sixty", 60, 0);
    write_album(&music, "Anchor one hundred twenty", 120, 1000);
    support::scan(&hito, &actor, &library).await;
    let server = AlbumProvider::start().await;
    associate_everything(&hito, &actor, &server).await;
    let meter = WorkMeter::install(&hito.catalog.store.pool).await;
    let mut anchor_work = Vec::new();
    for (title, tracks) in [("Anchor sixty", 60u64), ("Anchor one hundred twenty", 120)] {
        let id = release_id(&hito, &actor, title).await;
        let page = |offset| Browse {
            offset: Some(offset),
            limit: Some(PAGE),
            ..Default::default()
        };
        let (first, with_anchor) = meter
            .measure(hito.catalog.release(&actor, &id, page(0)))
            .await;
        let (later, without_anchor) = meter
            .measure(hito.catalog.release(&actor, &id, page(PAGE)))
            .await;
        assert_eq!(first.unwrap().tracks.items.len(), PAGE as usize);
        assert_eq!(later.unwrap().tracks.items.len(), PAGE as usize);
        let extra = with_anchor.saturating_sub(without_anchor);
        eprintln!(
            "anchor work tracks={tracks} first={with_anchor} later={without_anchor} extra={extra}"
        );
        assert!(
            extra < FIXED_WORK + tracks * ANCHOR_WORK_PER_TRACK,
            "the anchor read must stay linear in album size"
        );
        anchor_work.push(extra.max(1));
    }
    assert!(
        anchor_work[1] < anchor_work[0] * 3,
        "doubling the album must not triple the anchor work"
    );
    meter.remove(&hito.catalog.store.pool).await;
    drop(server);
}

async fn associate_everything(hito: &Services, actor: &crate::Actor, server: &AlbumProvider) {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Anchor work catalog".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = hito
        .plugins
        .install(
            actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let selected = MetadataProvider::HttpPlugin {
        installation_id: installation.id,
    };
    let current = hito.catalog.metadata_settings(actor).await.unwrap();
    let settings = hito
        .catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: current.revision,
                enabled: true,
                providers: vec![selected.clone()],
            },
        )
        .await
        .unwrap();
    let artist = hito
        .catalog
        .artists(actor, Browse::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let recordings = hito
        .catalog
        .recordings(
            actor,
            Search {
                limit: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(recordings.total, 180);
    for recording in recordings.items {
        let observation = hito
            .catalog
            .recording_metadata(actor, &recording.id, Default::default())
            .await
            .unwrap()
            .selected_observation
            .unwrap();
        hito.catalog
            .associate_recording_artist(
                actor,
                &recording.id,
                AssociateRecordingArtist {
                    artist_id: artist.clone(),
                    source_id: observation.source_id,
                    observation_id: observation.id,
                    role: ArtistCreditRole::RecordingArtist,
                    provider: selected.clone(),
                    provider_artist_id: provider::ARTIST_ID.into(),
                    policy_revision: settings.revision,
                },
            )
            .await
            .unwrap();
    }
}
