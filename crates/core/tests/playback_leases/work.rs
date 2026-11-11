use crate as core;
#[path = "fixture.rs"]
pub(super) mod fixture;
use crate::{AudioRequest, OpenedAudio};
use fixture::Case;
use std::time::Duration;

#[tokio::test]
async fn original_expiry_is_not_renewed_by_status_and_retires_an_unpolled_body() {
    let f = Case::new().await;
    f.services.playback.leases.state.lock().unwrap().lifetime = Some(Duration::from_secs(2));
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .unwrap(),
        lease
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    f.services
        .playback
        .release_lease(&f.actor, lease.id)
        .await
        .unwrap();
    assert!(body.chunk().await.is_err());
    let fresh = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let opened = f
        .services
        .playback
        .open_lease_audio(&f.actor, fresh.id, AudioRequest::default())
        .await
        .unwrap();
    assert_eq!(fixture::content(opened).await, f.original);
    f.close().await;
}

#[tokio::test]
async fn five_pending_listeners_fit_and_account_and_global_pending_caps_cannot_oversubscribe() {
    let f = Case::new().await;
    let pause = std::sync::Arc::new(super::testing::AdmissionPause::default());
    f.services
        .playback
        .leases
        .state
        .lock()
        .unwrap()
        .admission_pause = Some(pause.clone());
    let mut requests = Vec::new();
    for _ in 0..5 {
        let access = f.services.playback.clone();
        let actor = f.actor.clone();
        let request = f.request();
        requests.push(tokio::spawn(async move {
            access.begin_lease(&actor, request).await
        }));
    }
    pause.entered(5).await;
    assert!(matches!(
        f.services.playback.begin_lease(&f.actor, f.request()).await,
        Err(crate::Error::Conflict(_))
    ));
    let user = f
        .services
        .identity
        .create_user(
            &f.actor,
            crate::Credentials {
                username: "pending-peer".into(),
                password: "owned pending peer password".into(),
            },
        )
        .await
        .unwrap();
    f.services
        .identity
        .set_library_access(&f.actor, &f.library, &user.id, true)
        .await
        .unwrap();
    let login = f
        .services
        .identity
        .login(crate::Credentials {
            username: "pending-peer".into(),
            password: "owned pending peer password".into(),
        })
        .await
        .unwrap();
    let peer = f
        .services
        .identity
        .authenticate(&login.token)
        .await
        .unwrap();
    for _ in 0..3 {
        let access = f.services.playback.clone();
        let actor = peer.clone();
        let request = f.request();
        requests.push(tokio::spawn(async move {
            access.begin_lease(&actor, request).await
        }));
    }
    pause.entered(3).await;
    assert!(matches!(
        f.services.playback.begin_lease(&peer, f.request()).await,
        Err(crate::Error::Conflict(_))
    ));
    pause.release(8);
    for request in requests {
        request.await.unwrap().unwrap();
    }
    f.close().await;
}

#[tokio::test]
async fn cancelling_creators_and_closing_while_admission_is_paused_settles_every_owner() {
    let f = Case::new().await;
    let pause = std::sync::Arc::new(super::testing::AdmissionPause::default());
    f.services
        .playback
        .leases
        .state
        .lock()
        .unwrap()
        .admission_pause = Some(pause.clone());
    let mut requests = Vec::new();
    for _ in 0..5 {
        let access = f.services.playback.clone();
        let actor = f.actor.clone();
        let request = f.request();
        requests.push(tokio::spawn(async move {
            access.begin_lease(&actor, request).await
        }));
    }
    pause.entered(5).await;
    for request in &requests {
        request.abort();
    }
    for request in requests {
        assert!(request.await.unwrap_err().is_cancelled());
    }
    f.services.identity.logout(&f.token).await.unwrap();
    f.services.playback.close().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn cancelled_read_and_cancelled_close_observer_still_join_the_file_worker() {
    let f = Case::new().await;
    let pause = std::sync::Arc::new(super::testing::ReadPause::default());
    f.services.playback.leases.state.lock().unwrap().read_pause = Some(pause.clone());
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let entry = f
        .services
        .playback
        .leases
        .entry(&f.actor, lease.id)
        .unwrap();
    assert_eq!(
        entry.bytes_read.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "admission only opens and reads metadata"
    );
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    let reader = tokio::spawn(async move { body.chunk().await });
    pause.entered().await;
    assert_eq!(
        entry.bytes_read.load(std::sync::atomic::Ordering::Relaxed),
        f.original.len() as u64
    );
    reader.abort();
    assert!(reader.await.unwrap_err().is_cancelled());
    let access = f.services.playback.clone();
    let closing = tokio::spawn(async move { access.close().await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !closing.is_finished(),
        "close must retain the blocked worker"
    );
    closing.abort();
    assert!(closing.await.unwrap_err().is_cancelled());
    let access = f.services.playback.clone();
    let final_close = tokio::spawn(async move { access.close().await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        !final_close.is_finished(),
        "dropping the first observer must not lose the join"
    );
    pause.release();
    final_close.await.unwrap().unwrap();
    drop(entry);
    f.close().await;
}

#[tokio::test]
async fn mutation_during_an_owned_read_is_rejected_before_the_chunk_is_published() {
    use std::io::{Seek, SeekFrom, Write};
    let f = Case::new().await;
    let pause = std::sync::Arc::new(super::testing::ReadPause::default());
    f.services.playback.leases.state.lock().unwrap().read_pause = Some(pause.clone());
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    let reader = tokio::spawn(async move { body.chunk().await });
    pause.entered().await;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(f.path())
        .unwrap();
    file.seek(SeekFrom::Start(100)).unwrap();
    file.write_all(&[1, 2, 3, 4]).unwrap();
    file.sync_all().unwrap();
    pause.release();
    assert!(
        reader.await.unwrap().is_err(),
        "already-read bytes must not escape the failed post-read fence"
    );
    f.close().await;
}

#[tokio::test]
async fn cancelled_create_after_file_open_closes_the_unpublished_descriptor() {
    let f = Case::new().await;
    let pause = std::sync::Arc::new(super::testing::AdmissionPause::default());
    f.services
        .playback
        .leases
        .state
        .lock()
        .unwrap()
        .publication_pause = Some(pause.clone());
    let access = f.services.playback.clone();
    let actor = f.actor.clone();
    let request = f.request();
    let creating = tokio::spawn(async move { access.begin_lease(&actor, request).await });
    pause.entered(1).await;
    #[cfg(target_os = "linux")]
    assert!(
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| std::fs::read_link(entry.path()).ok())
            .any(|path| path == f.path()),
        "the test pauses after the owned descriptor is captured"
    );
    creating.abort();
    assert!(creating.await.unwrap_err().is_cancelled());
    f.services.playback.close().await.unwrap();
    #[cfg(target_os = "linux")]
    assert!(
        !std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| std::fs::read_link(entry.path()).ok())
            .any(|path| path == f.path()),
        "cancelled publication cannot leak the descriptor"
    );
    f.close().await;
}

#[tokio::test]
async fn pending_limits_include_session_sql_and_close_settles_waiting_creators() {
    let f = Case::new().await;
    let pool = f.services.playback.store.pool.clone();
    let mut held = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        held.push(pool.acquire().await.unwrap());
    }
    let mut calls = (0..5)
        .map(|_| {
            Some(Box::pin(
                f.services.playback.begin_lease(&f.actor, f.request()),
            ))
        })
        .collect::<Vec<_>>();
    for call in &mut calls {
        assert!(
            poll_once(call.as_mut().unwrap().as_mut())
                .await
                .is_pending()
        );
    }
    tokio::task::yield_now().await;
    let sixth = tokio::time::timeout(
        Duration::from_millis(200),
        f.services.playback.begin_lease(&f.actor, f.request()),
    )
    .await;
    let closed =
        tokio::time::timeout(Duration::from_millis(500), f.services.playback.close()).await;
    let mut settled = true;
    for slot in &mut calls {
        match poll_once(slot.as_mut().unwrap().as_mut()).await {
            std::task::Poll::Ready(result) => {
                assert!(result.is_err());
                *slot = None;
            }
            std::task::Poll::Pending => settled = false,
        }
    }
    drop(held);
    for call in calls.into_iter().flatten() {
        assert!(call.await.is_err());
    }
    drop(pool);
    f.close().await;
    assert!(
        matches!(sixth, Ok(Err(crate::Error::Conflict(_)))),
        "pending admission must be reserved before waiting for session SQL"
    );
    assert!(
        matches!(closed, Ok(Ok(()))),
        "close must cancel and join pending admission"
    );
    assert!(
        settled,
        "close cannot acknowledge while creators still wait outside its ownership"
    );
}

pub(super) async fn poll_once<F: std::future::Future>(
    mut future: std::pin::Pin<&mut F>,
) -> std::task::Poll<F::Output> {
    std::future::poll_fn(|context| std::task::Poll::Ready(future.as_mut().poll(context))).await
}
