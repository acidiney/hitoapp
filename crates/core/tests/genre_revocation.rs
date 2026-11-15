#[path = "fixtures/playlist_read_barrier.rs"]
mod barrier;
use hito_core::test_support::{hang_guard::hang_guard, support};

use barrier::HeaderBarrier;
use hito_core::{Actor, Credentials, Error, Services};
use lofty::{
    prelude::{Accessor, TagExt},
    tag::{Tag, TagType},
};
use std::{future::Future, sync::OnceLock, time::Duration};
use tracing_subscriber::prelude::*;

struct Barriers {
    tiles: HeaderBarrier,
    detail: HeaderBarrier,
    tracks: HeaderBarrier,
}

fn barriers() -> &'static Barriers {
    static BARRIERS: OnceLock<Barriers> = OnceLock::new();
    BARRIERS.get_or_init(|| {
        let barriers = Barriers {
            tiles: HeaderBarrier::matching(
                "AS track_count FROM keys WHERE k IS NOT NULL AND label IS NOT NULL",
                "ORDER BY track_count DESC, k LIMIT ",
            ),
            detail: HeaderBarrier::matching(
                "SELECT release_id FROM (SELECT id, release_id FROM (",
                "WHERE release_id IS NOT NULL GROUP BY release_id",
            ),
            tracks: HeaderBarrier::matching(
                "SELECT id, title FROM (",
                ") GROUP BY id ORDER BY title COLLATE NOCASE, id LIMIT ",
            ),
        };
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry()
                .with(barriers.tiles.clone())
                .with(barriers.detail.clone())
                .with(barriers.tracks.clone()),
        )
        .unwrap();
        barriers
    })
}

async fn tagged_library(host: &str) -> (tempfile::TempDir, Services, Actor, String) {
    let (directory, hito, setup_actor, library, music) = support::library(host).await;
    let path = music.join("owned.wav");
    support::write_wav(&path, 7);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_title("Owned".into());
    tag.set_artist("Performer".into());
    tag.set_album("Owned Album".into());
    tag.set_genre("Fado".into());
    tag.save_to_path(&path, Default::default()).unwrap();
    support::scan(&hito, &setup_actor, &library).await;
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    (directory, hito, actor, session.token)
}

async fn close_owned_services(hito: &Services) -> Vec<&'static str> {
    let deadline = Duration::from_secs(5);
    let mut failed = Vec::new();
    if !matches!(
        tokio::time::timeout(deadline, hito.playback.close()).await,
        Ok(Ok(()))
    ) {
        failed.push("playback");
    }
    if !matches!(
        tokio::time::timeout(deadline, hito.replicas.close()).await,
        Ok(Ok(()))
    ) {
        failed.push("replicas");
    }
    if !matches!(
        tokio::time::timeout(deadline, hito.catalog_replication.close()).await,
        Ok(Ok(()))
    ) {
        failed.push("catalog replication");
    }
    failed
}

async fn assert_rejected_after_concurrent_logout<T, F>(
    hito: &Services,
    token: &str,
    held: &HeaderBarrier,
    read: F,
) where
    T: Send + 'static,
    F: Future<Output = hito_core::Result<T>> + Send + 'static,
{
    let release_on_drop = held.arm();
    let mut reading = tokio::spawn(read);
    let entered = tokio::time::timeout(Duration::from_secs(2), held.entered()).await;
    let logout = match entered {
        Ok(()) => {
            Some(tokio::time::timeout(Duration::from_secs(3), hito.identity.logout(token)).await)
        }
        Err(_) => None,
    };
    drop(release_on_drop);
    let read = match tokio::time::timeout(Duration::from_secs(2), &mut reading).await {
        Ok(joined) => Some(joined.unwrap()),
        Err(_) => {
            reading.abort();
            let _ = reading.await;
            None
        }
    };
    let unclosed = close_owned_services(hito).await;

    entered.expect("The genre read must reach its SQL completion barrier");
    let read = read.expect("The genre reader must finish after its statement is released");
    assert!(
        !held.timed_out(),
        "The SQL completion barrier must be released by the test, not its deadline"
    );
    logout
        .expect("Logout runs once the read holds its snapshot")
        .expect("Real logout must commit while the old genre snapshot is held")
        .unwrap();
    assert!(
        matches!(read, Err(Error::Unauthorized)),
        "A genre read must reject a session revoked after its initial authorization snapshot"
    );
    assert!(
        unclosed.is_empty(),
        "Owned services must close: {unclosed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn genre_tiles_recheck_their_session_after_concurrent_logout() {
    hang_guard("the genre tiles revocation test", async {
        let held = &barriers().tiles;
        let (_directory, hito, actor, token) = tagged_library("genre-tiles-revocation").await;
        assert_eq!(hito.catalog.genres(&actor).await.unwrap().len(), 1);
        let catalog = hito.catalog.clone();
        assert_rejected_after_concurrent_logout(&hito, &token, held, async move {
            catalog.genres(&actor).await
        })
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_genre_page_rechecks_its_session_after_concurrent_logout() {
    hang_guard("the genre page revocation test", async {
        let held = &barriers().detail;
        let (_directory, hito, actor, token) = tagged_library("genre-detail-revocation").await;
        assert_eq!(
            hito.catalog
                .genre(&actor, "fado")
                .await
                .unwrap()
                .tracks
                .len(),
            1
        );
        let catalog = hito.catalog.clone();
        assert_rejected_after_concurrent_logout(&hito, &token, held, async move {
            catalog.genre(&actor, "fado").await
        })
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_genre_tracks_page_rechecks_its_session_after_concurrent_logout() {
    hang_guard("the genre tracks revocation test", async {
        let held = &barriers().tracks;
        let (_directory, hito, actor, token) = tagged_library("genre-tracks-revocation").await;
        let baseline = hito
            .catalog
            .genre_tracks(&actor, "fado", None, None)
            .await
            .unwrap();
        assert_eq!(baseline.items.len(), 1);
        let catalog = hito.catalog.clone();
        assert_rejected_after_concurrent_logout(&hito, &token, held, async move {
            catalog.genre_tracks(&actor, "fado", None, None).await
        })
        .await;
    })
    .await;
}
