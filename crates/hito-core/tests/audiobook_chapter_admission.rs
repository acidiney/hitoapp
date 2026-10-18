#[path = "fixtures/audiobooks_library.rs"]
mod library;
#[path = "fixtures/audiobooks_peer.rs"]
mod peer;
#[path = "fixtures/playback.rs"]
mod playback;
mod support;

use hito_core::{Action, Actor, AudiobookChapterAnchor, Credentials, Error, Services};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[tokio::test]
async fn chapter_admission_is_bounded_private_current_and_read_only_for_a_full_book() {
    let library = library::Library::prepare().await;
    let peer = peer::Peer::prepare(&library).await;
    let mut anchors = library
        .entries
        .into_iter()
        .zip(library.recordings)
        .map(|(entry_id, recording_id)| AudiobookChapterAnchor {
            audiobook_id: library.playlist,
            entry_id,
            recording_id,
        })
        .collect::<Vec<_>>();
    {
        let mut inserts = (2..10_000).map(|index| {
            let chapter = AudiobookChapterAnchor {
                audiobook_id: library.playlist,
                entry_id: Uuid::new_v4(),
                recording_id: library.recordings[index % 2],
            };
            anchors.push(chapter);
            Action::EntryInsert {
                playlist_id: library.playlist,
                entry_id: chapter.entry_id,
                recording_id: chapter.recording_id,
                position: (index as i64 + 1) * 10,
            }
        });
        loop {
            let actions = inserts.by_ref().take(100).collect::<Vec<_>>();
            if actions.is_empty() {
                break;
            }
            library::apply(&library.hito, &library.actor, actions).await;
        }
    }
    let deleted = Uuid::new_v4();
    let deleted_entry = Uuid::new_v4();
    library::apply(
        &library.hito,
        &library.actor,
        vec![
            Action::AudiobookCreate {
                playlist_id: library.playlist,
            },
            Action::AudiobookChapterSet {
                chapter: anchors[9_999],
            },
            Action::PlaylistCreate {
                playlist_id: deleted,
                name: "Private deleted admission book".into(),
            },
            Action::EntryInsert {
                playlist_id: deleted,
                entry_id: deleted_entry,
                recording_id: library.recordings[0],
                position: 0,
            },
            Action::AudiobookCreate {
                playlist_id: deleted,
            },
            Action::PlaylistDelete {
                playlist_id: deleted,
            },
        ],
    )
    .await;
    let before = snapshot(&library, &peer).await;
    assert_eq!(before["book"]["chapters"].as_array().unwrap().len(), 10_000);
    assert_eq!(
        before["book"]["continuation"]["chapter"],
        json!(anchors[9_999])
    );
    assert!(!library.actor.user().admin && !peer.actor.user().admin);
    let distinct = anchors.iter().step_by(10).copied().collect::<Vec<_>>();
    let repeated = vec![anchors[9_999]; 1000];
    assert_eq!(distinct.len(), 1000);
    for requested in [&[][..], distinct.as_slice(), repeated.as_slice()] {
        library
            .hito
            .collection
            .check_audiobook_chapters(&library.actor, requested)
            .await
            .unwrap();
    }
    assert!(matches!(
        library
            .hito
            .collection
            .check_audiobook_chapters(&library.actor, &vec![anchors[0]; 1001])
            .await,
        Err(Error::Invalid(_))
    ));
    for invalid in [
        AudiobookChapterAnchor {
            audiobook_id: Uuid::nil(),
            ..anchors[0]
        },
        AudiobookChapterAnchor {
            entry_id: Uuid::nil(),
            ..anchors[0]
        },
        AudiobookChapterAnchor {
            recording_id: Uuid::nil(),
            ..anchors[0]
        },
    ] {
        assert!(matches!(
            library
                .hito
                .collection
                .check_audiobook_chapters(&library.actor, &[anchors[0], invalid])
                .await,
            Err(Error::Invalid(_))
        ));
    }
    let foreign = AudiobookChapterAnchor {
        audiobook_id: peer.book,
        entry_id: peer.entry,
        recording_id: library.recordings[0],
    };
    library
        .hito
        .collection
        .check_audiobook_chapters(&peer.actor, &[foreign])
        .await
        .unwrap();
    for (actor, invalid) in [
        (&library.actor, foreign),
        (&peer.actor, anchors[0]),
        (
            &library.actor,
            AudiobookChapterAnchor {
                audiobook_id: Uuid::new_v4(),
                ..anchors[0]
            },
        ),
        (
            &library.actor,
            AudiobookChapterAnchor {
                entry_id: Uuid::new_v4(),
                ..anchors[0]
            },
        ),
        (
            &library.actor,
            AudiobookChapterAnchor {
                recording_id: library.recordings[1],
                ..anchors[0]
            },
        ),
        (
            &library.actor,
            AudiobookChapterAnchor {
                audiobook_id: deleted,
                entry_id: deleted_entry,
                ..anchors[0]
            },
        ),
    ] {
        let valid = if actor.user().id == peer.actor.user().id {
            foreign
        } else {
            anchors[0]
        };
        assert!(matches!(
            library
                .hito
                .collection
                .check_audiobook_chapters(actor, &[valid, invalid])
                .await,
            Err(Error::NotFound)
        ));
    }
    assert_eq!(snapshot(&library, &peer).await, before);
    let session = library
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let admin = library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let shared = library
        .hito
        .catalog
        .libraries(&library.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|item| !item.personal)
        .unwrap();
    library
        .hito
        .identity
        .set_library_access(&admin, &shared.id, &library.actor.user().id, false)
        .await
        .unwrap();
    library
        .hito
        .collection
        .check_audiobook_chapters(&library.actor, &[])
        .await
        .unwrap();
    assert!(matches!(
        library
            .hito
            .collection
            .check_audiobook_chapters(&library.actor, &repeated)
            .await,
        Err(Error::NotFound)
    ));
    library
        .hito
        .collection
        .check_audiobook_chapters(&peer.actor, &[foreign])
        .await
        .unwrap();
    assert_eq!(
        events(&library.hito, &library.actor).await,
        before["events"]
    );
    library
        .hito
        .identity
        .set_library_access(&admin, &shared.id, &library.actor.user().id, true)
        .await
        .unwrap();
    library
        .hito
        .collection
        .check_audiobook_chapters(&library.actor, &distinct)
        .await
        .unwrap();
    assert_eq!(snapshot(&library, &peer).await, before);
    library.assert_no_downloads().await;
    peer.assert_no_downloads(&library).await;
    library.assert_originals_unchanged();
    let reopened = library.reopen().await;
    reopened
        .hito
        .collection
        .check_audiobook_chapters(&reopened.actor, &repeated)
        .await
        .unwrap();
    assert_eq!(snapshot(&reopened, &peer).await, before);
    reopened.assert_no_downloads().await;
    peer.assert_no_downloads(&reopened).await;
    reopened.assert_originals_unchanged();
}

#[tokio::test]
async fn revoked_and_expired_sessions_cannot_check_even_an_empty_chapter_batch() {
    let f = playback::LocalPlayback::start().await;
    let chapter = AudiobookChapterAnchor {
        audiobook_id: Uuid::new_v4(),
        entry_id: Uuid::new_v4(),
        recording_id: f.recording.parse().unwrap(),
    };
    support::synchronize(
        &f.services,
        &f.actor,
        vec![
            Action::PlaylistCreate {
                playlist_id: chapter.audiobook_id,
                name: "Private session admission book".into(),
            },
            Action::EntryInsert {
                playlist_id: chapter.audiobook_id,
                entry_id: chapter.entry_id,
                recording_id: chapter.recording_id,
                position: 0,
            },
            Action::AudiobookCreate {
                playlist_id: chapter.audiobook_id,
            },
            Action::ProgressSet {
                recording_id: chapter.recording_id,
                position_ms: 250,
                speed: 1.25,
            },
            Action::AudiobookChapterSet { chapter },
        ],
    )
    .await;
    let before = json!({
        "book": f.services.collection.audiobook(&f.actor, &chapter.audiobook_id.to_string()).await.unwrap(),
        "progress": f.services.collection.saved_playback(&f.actor, &[chapter.recording_id]).await.unwrap(),
        "events": f.services.collection.events(&f.actor, 0).await.unwrap()
    });
    let credentials = || Credentials {
        username: "admin".into(),
        password: "indexing test password".into(),
    };
    let revoked = f.services.identity.login(credentials()).await.unwrap();
    let revoked_actor = f
        .services
        .identity
        .authenticate(&revoked.token)
        .await
        .unwrap();
    f.services.identity.logout(&revoked.token).await.unwrap();
    for requested in [&[][..], &[chapter][..]] {
        assert!(matches!(
            f.services
                .collection
                .check_audiobook_chapters(&revoked_actor, requested)
                .await,
            Err(Error::Unauthorized)
        ));
    }
    let expired = f.services.identity.login(credentials()).await.unwrap();
    let expired_actor = f
        .services
        .identity
        .authenticate(&expired.token)
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE sessions SET expires_at=unixepoch()-1 WHERE user_id=?")
        .bind(expired_actor.user().id.clone())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    for requested in [&[][..], &[chapter][..]] {
        assert!(matches!(
            f.services
                .collection
                .check_audiobook_chapters(&expired_actor, requested)
                .await,
            Err(Error::Unauthorized)
        ));
    }
    let fresh = f.services.identity.login(credentials()).await.unwrap();
    let fresh_actor = f
        .services
        .identity
        .authenticate(&fresh.token)
        .await
        .unwrap();
    f.services
        .collection
        .check_audiobook_chapters(&fresh_actor, &[chapter])
        .await
        .unwrap();
    assert_eq!(
        json!({
            "book": f.services.collection.audiobook(&fresh_actor, &chapter.audiobook_id.to_string()).await.unwrap(),
            "progress": f.services.collection.saved_playback(&fresh_actor, &[chapter.recording_id]).await.unwrap(),
            "events": f.services.collection.events(&fresh_actor, 0).await.unwrap()
        }),
        before
    );
    assert!(matches!(
        f.services
            .downloads
            .status(&fresh_actor, &f.recording)
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        std::fs::read(f.music.join("track.wav")).unwrap(),
        f.original
    );
}

async fn snapshot(library: &library::Library, peer: &peer::Peer) -> Value {
    json!({
        "book": library.hito.collection.audiobook(&library.actor, &library.playlist.to_string()).await.unwrap(),
        "musical": library.snapshot().await,
        "events": events(&library.hito, &library.actor).await,
        "peer": peer.snapshot(library).await
    })
}

async fn events(hito: &Services, actor: &Actor) -> Value {
    let mut cursor = 0;
    let mut count = 0;
    let mut digest = Sha256::new();
    loop {
        let page = hito.collection.events(actor, cursor).await.unwrap();
        for operation in &page.operations {
            let bytes = serde_json::to_vec(operation).unwrap();
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
            count += 1;
        }
        assert!(!page.has_more || page.cursor > cursor);
        cursor = page.cursor;
        if !page.has_more {
            break;
        }
    }
    json!({"cursor": cursor, "count": count, "digest": format!("{:x}", digest.finalize())})
}
