use hito_core::test_support::artist_identity_selection_catalog as catalog;
#[allow(dead_code)]
#[path = "../fixtures/artist_identity_selection_library.rs"]
mod library;
use hito_core::test_support::support;

use futures_util::FutureExt;
use hito_core::{Actor, Artist, Browse, ChooseRecordingArtistIdentity, MetadataProvider};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use sqlx::sqlite::SqliteConnectOptions;
use std::panic::{AssertUnwindSafe, resume_unwind};
use uuid::Uuid;

const OTHER: &str = "Other Performer";

#[tokio::test]
async fn coverage_needs_the_credits_own_chosen_recordings_and_survives_loose_recordings() {
    let mut library = library::Library::prepare().await;
    let result = AssertUnwindSafe(exercise(&library)).catch_unwind().await;
    library.origin.shutdown().await;
    library.catalog.finish().await;
    if let Err(panic) = result {
        resume_unwind(panic);
    }
}

async fn credits(library: &library::Library, actor: &Actor, name: &str) -> Vec<Artist> {
    library
        .hito
        .catalog
        .artists(
            actor,
            Browse {
                q: Some(name.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .into_iter()
        .filter(|artist| artist.name == name)
        .collect()
}

async fn choose(
    library: &library::Library,
    actor: &Actor,
    provider: &MetadataProvider,
    recording: &str,
    identity: &str,
) {
    let hito = &library.hito;
    let context = hito
        .catalog
        .begin_artist_review(actor, recording, provider.clone())
        .await
        .unwrap();
    let association = hito
        .catalog
        .confirm_artist_review(actor, &context, identity)
        .await
        .unwrap();
    let choice = hito
        .catalog
        .recording_artist_identity_choice(actor, recording)
        .await
        .unwrap();
    hito.catalog
        .choose_recording_artist_identity(
            actor,
            recording,
            ChooseRecordingArtistIdentity {
                request_id: Uuid::new_v4(),
                expected_revision: choice.revision,
                association_id: Uuid::parse_str(&association.id).unwrap(),
                context,
            },
        )
        .await
        .unwrap();
}

async fn exercise(library: &library::Library) {
    let hito = &library.hito;
    let actor = &library.actor;
    let provider = &library.metadata_provider;
    let collab = library.music().join("collab.wav");
    support::write_wav(&collab, 1);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Collaboration track".into());
    tag.set_artist(OTHER.into());
    tag.set_album("Collaboration album".into());
    tag.insert_text(ItemKey::AlbumArtist, catalog::CREDIT.into());
    tag.save_to_path(&collab, Default::default()).unwrap();
    support::scan(hito, actor, &library.shared).await;
    let other = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|recording| recording.title == "Collaboration track")
        .unwrap()
        .id;
    assert_eq!(credits(library, actor, catalog::CREDIT).await.len(), 2);

    let pool =
        sqlx::SqlitePool::connect_with(SqliteConnectOptions::new().filename(library.database()))
            .await
            .unwrap();
    sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,ready) VALUES ('loose-source',?,'plugin','loose',0)")
        .bind(&library.namesake)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms) VALUES ('loose-source',NULL,'Loose','Loose',0)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    choose(library, actor, provider, &library.local, catalog::ALPHA).await;
    choose(library, actor, provider, &library.namesake, catalog::BETA).await;
    let shared = |credits: Vec<Artist>| {
        credits
            .into_iter()
            .filter(|artist| artist.library_id == library.shared)
            .count()
    };
    assert_eq!(
        shared(credits(library, actor, catalog::CREDIT).await),
        1,
        "A chosen recording without a release must not make an uncovered album credit look covered"
    );

    choose(library, actor, provider, &other, catalog::BETA).await;
    assert_eq!(
        shared(credits(library, actor, catalog::CREDIT).await),
        1,
        "An album-artist credit stays listed when the only chosen track on its album belongs to another credit"
    );
    assert!(
        credits(library, actor, OTHER).await.is_empty(),
        "The other performer's own chosen recording and album cover its credit"
    );

    hito.identity
        .set_library_access(actor, &library.shared, &actor.user().id, false)
        .await
        .unwrap();
    assert_eq!(shared(credits(library, actor, catalog::CREDIT).await), 0);
    hito.identity
        .set_library_access(actor, &library.shared, &actor.user().id, true)
        .await
        .unwrap();
    assert_eq!(shared(credits(library, actor, catalog::CREDIT).await), 1);
    assert!(credits(library, actor, OTHER).await.is_empty());

    let peer = &library.peer;
    assert_eq!(
        shared(credits(library, peer, OTHER).await),
        1,
        "The owner's choices must not hide the peer's credits"
    );
    assert_eq!(shared(credits(library, peer, catalog::CREDIT).await), 1);
}
