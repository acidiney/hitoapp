use hito_core::test_support::support;

use hito_core::{Action, Browse, Search};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use uuid::Uuid;

#[tokio::test]
async fn a_personal_credit_keeps_its_id_when_a_source_later_uses_the_same_name() {
    let (_dir, hito, actor, library, music) = support::library("credit-identity-host").await;
    let path = music.join("track.wav");
    support::write_wav(&path, 0);
    support::scan(&hito, &actor, &library).await;
    let track = hito
        .catalog
        .recordings(&actor, Search::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording_id = Uuid::parse_str(&track.id).unwrap();
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingArtistSet {
            recording_id,
            artist: "Later observed credit".into(),
        }],
    )
    .await;
    let credit = hito
        .catalog
        .artists(
            &actor,
            Browse {
                q: Some("Later observed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .remove(0);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_artist("Later observed credit".into());
    tag.save_to_path(&path, Default::default()).unwrap();
    support::scan(&hito, &actor, &library).await;
    support::synchronize(
        &hito,
        &actor,
        vec![Action::RecordingArtistInherit { recording_id }],
    )
    .await;
    let inherited = hito
        .catalog
        .artists(
            &actor,
            Browse {
                q: Some("Later observed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(inherited.total, 1);
    assert_eq!(inherited.items[0].id, credit.id);
    assert_eq!(
        hito.catalog
            .artist(&actor, &credit.id, Default::default())
            .await
            .unwrap()
            .releases
            .total,
        1
    );
}
