use crate::{
    Actor, ArtistAssociation, ArtistCreditRole, AssociateRecordingArtist, Browse,
    ChooseAlbumEdition, MetadataProvider, Search, Services, UpdateMetadataSettings,
    test_support::support,
};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use std::path::Path;
use uuid::Uuid;

pub(super) fn write_album(music: &Path, album: &str, offset: i16, tracks: i16) {
    for track in 0..tracks {
        let path = music.join(format!("{album}-{track}.wav"));
        support::write_wav(&path, offset + track + 1);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title(format!("{album} song {track}"));
        tag.set_artist("Performer".into());
        tag.set_album(album.into());
        tag.save_to_path(&path, Default::default()).unwrap();
    }
}

pub(super) async fn enable_builtin(hito: &Services, actor: &Actor) -> i64 {
    let settings = hito.catalog.metadata_settings(actor).await.unwrap();
    hito.catalog
        .set_metadata_settings(
            actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: true,
                providers: vec![MetadataProvider::BuiltinMusicBrainz],
            },
        )
        .await
        .unwrap()
        .revision
}

pub(super) async fn recording_of(hito: &Services, actor: &Actor, album: &str) -> (String, String) {
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
    let recording = recordings
        .items
        .into_iter()
        .find(|recording| recording.title.starts_with(&format!("{album} song 0")))
        .unwrap()
        .id;
    let release = hito
        .catalog
        .releases(
            actor,
            Browse {
                limit: Some(200),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|release| release.title == album)
        .unwrap()
        .id;
    (recording, release)
}

pub(super) async fn associate(
    hito: &Services,
    actor: &Actor,
    recording: &str,
    revision: i64,
) -> ArtistAssociation {
    let artist = hito
        .catalog
        .artists(actor, Browse::default())
        .await
        .unwrap()
        .items[0]
        .id
        .clone();
    let observation = hito
        .catalog
        .recording_metadata(actor, recording, Default::default())
        .await
        .unwrap()
        .selected_observation
        .unwrap();
    hito.catalog
        .associate_recording_artist(
            actor,
            recording,
            AssociateRecordingArtist {
                artist_id: artist,
                source_id: observation.source_id,
                observation_id: observation.id,
                role: ArtistCreditRole::RecordingArtist,
                provider: MetadataProvider::BuiltinMusicBrainz,
                provider_artist_id: Uuid::new_v4().to_string(),
                policy_revision: revision,
            },
        )
        .await
        .unwrap()
}

pub(super) async fn choose(
    hito: &Services,
    actor: &Actor,
    release: &str,
    recording: &str,
    association: &str,
    group: &str,
) {
    hito.catalog
        .choose_album_edition(
            actor,
            release,
            ChooseAlbumEdition {
                request_id: Uuid::new_v4(),
                expected_revision: 0,
                recording_id: recording.into(),
                association_id: association.into(),
                release_group_id: group.into(),
                edition_id: format!("edition-{group}"),
            },
        )
        .await
        .unwrap();
}
