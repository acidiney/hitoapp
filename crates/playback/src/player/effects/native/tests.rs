use super::super::sources::Sources;
use super::*;
use hito_core::test_support::hang_guard::hang_guard;
use hito_core::{
    AudioRequest, Credentials, PlaybackFailureReason, Search,
    test_support::{provider_audio::AudioLibrary, support},
};
use tokio::sync::oneshot;

#[tokio::test]
async fn persistent_provider_failures_open_two_sources_then_one_per_retry_without_refreshing() {
    let library = hang_guard(
        "owned persistent provider failure fixture",
        AudioLibrary::start(),
    )
    .await;
    assert_eq!(library.server.requests("v1/sources").await.len(), 1);
    library
        .server
        .audio(
            axum::http::StatusCode::BAD_GATEWAY,
            Default::default(),
            Vec::new(),
        )
        .await;
    let key = Key {
        scope: uuid::Uuid::new_v4(),
        selection: 1,
        entry: uuid::Uuid::new_v4(),
        start: None,
    };
    let mut sources = Sources::new(library.hito.catalog.clone(), library.hito.playback.clone());
    sources.reset(Some(key));
    assert!(sources.request(&library.actor, &library.source.recording_id));
    let demand = sources
        .prepare_request(&library.actor, &library.source.recording_id)
        .unwrap();
    let started = tokio::time::Instant::now();

    for (attempt, expected_audio_total, expected_opens) in [(1, 2, 2), (2, 3, 1), (3, 4, 1)] {
        if attempt > 1 {
            assert!(sources.restart_attempt());
            assert!(
                sources.prepare_request(&library.actor, &library.source.recording_id)
                    == Some(demand),
                "retries must retain the original acquisition demand"
            );
        }
        let mut opened = 0;
        let failure = loop {
            match sources.next_with_attempt(Some((attempt, started))).await {
                Ok(Some(grant)) => {
                    assert!(
                        grant.source_id() == library.source.id,
                        "each pass must select the owned source"
                    );
                    let error = match grant.open(AudioRequest::default()).await {
                        Ok(_) => panic!("the owned provider must keep returning 502"),
                        Err(error) => error,
                    };
                    let retryable = matches!(
                        &error,
                        hito_core::Error::PlaybackProvider {
                            reason: PlaybackFailureReason::ProviderUnavailable,
                            ..
                        }
                    );
                    assert!(retryable, "the real provider failure must be retryable");
                    sources.failed(PlayerError::from(error), retryable);
                    opened += 1;
                    let requests = library.server.requests("v1/audio").await;
                    let total = match attempt {
                        1 => opened,
                        2 => 2 + opened,
                        _ => 3 + opened,
                    };
                    assert_eq!(requests.len(), total);
                    assert!(
                        requests.iter().all(|(_, query)| {
                            query["_method"] == "GET"
                                && query["source_id"].as_str()
                                    == Some("audio:/1?private=value&other=bad")
                        }),
                        "each open must remain a GET for the owned source"
                    );
                    let expected_sources = if attempt == 1 && opened == 1 { 1 } else { 2 };
                    assert_eq!(
                        library.server.requests("v1/sources").await.len(),
                        expected_sources
                    );
                }
                Ok(None) => panic!("an available owned source must be selected"),
                Err(failure) => break failure,
            }
        };
        assert_eq!(opened, expected_opens);
        assert_eq!(failure.error, PlayerError::Playback);
        assert!(failure.retryable);
        assert!(!failure.cancelled);
        assert_eq!(
            library.server.requests("v1/audio").await.len(),
            expected_audio_total
        );
        assert_eq!(library.server.requests("v1/sources").await.len(), 2);
    }
    assert!(
        !sources.has_demand(),
        "the third pass must leave no demand for a fourth pass"
    );
    sources.close().await;
}

#[test]
fn only_an_output_format_failure_has_its_own_player_error() {
    assert_eq!(
        failure_of(Some(crate::Error::Native(hito_audio::Error::OutputFormat))),
        PlayerError::OutputFormat
    );
    for other in [
        Some(crate::Error::Native(hito_audio::Error::Decode)),
        Some(crate::Error::Native(hito_audio::Error::OutputUnavailable)),
        Some(crate::Error::Input),
        None,
    ] {
        assert_eq!(failure_of(other), PlayerError::Playback);
    }
}

#[tokio::test]
async fn deadline_failure_uses_fresh_catalog_access_before_publishing() {
    let library = hang_guard("owned fresh finalizer fixture", AudioLibrary::start()).await;
    let session = library
        .hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let recording = uuid::Uuid::parse_str(&library.source.recording_id).unwrap();
    let key = Key {
        scope: uuid::Uuid::new_v4(),
        selection: 1,
        entry: uuid::Uuid::new_v4(),
        start: None,
    };
    let id = FinalizationId {
        key,
        demand: uuid::Uuid::new_v4(),
        play_revision: 1,
        seek_id: None,
        restoration_id: None,
    };
    let mut native = Native::new(library.hito.catalog.clone(), library.hito.playback.clone());
    native.target = Some(Target {
        key,
        actor: actor.clone(),
        recording,
        playing: true,
        play_revision: 1,
        seek: None,
        restoration: None,
        output: true,
        volume: 1000,
        speed: 1000,
        permitted: true,
        next: None,
    });
    native.deadline_pending = Some(id);
    let catalog = library.hito.catalog.clone();
    let (entered, entered_rx) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    native.spawn_finalization(id, async { true }, async move {
        let _ = entered.send(());
        let _ = release_rx.await;
        catalog
            .recordings_by_ids(&actor, &[recording])
            .await
            .map(|_| ())
            .map_err(PlayerError::from)
    });
    entered_rx.await.unwrap();
    library.hito.identity.logout(&session.token).await.unwrap();
    release.send(()).unwrap();
    let Some(FinalizationTaskResult::Finished(result)) = native.finalizing.join_next().await else {
        panic!("the held finalizer must complete with its fresh access result");
    };
    let update = native.finish_finalization(result).unwrap();
    assert_eq!(update.error, Some(PlayerError::Unauthorized));
    assert!(!update.exhausted);
    assert!(update.key == key);
    native.close().await;
}

#[tokio::test]
async fn deadline_failure_does_not_skip_after_fresh_library_denial() {
    let library = hang_guard("owned denied finalizer fixture", AudioLibrary::start()).await;
    let shared = library
        .hito
        .catalog
        .libraries(&library.actor)
        .await
        .unwrap()
        .into_iter()
        .find(|library| !library.personal)
        .unwrap();
    let path = library.music.join("member-track.wav");
    support::write_wav(&path, 100);
    support::scan(&library.hito, &library.actor, &shared.id).await;
    let recording = library
        .hito
        .catalog
        .recordings(
            &library.actor,
            Search {
                q: Some("member-track".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .items
        .into_iter()
        .find(|recording| recording.title == "member-track")
        .unwrap();
    let user = library
        .hito
        .identity
        .create_user(
            &library.actor,
            Credentials {
                username: "owned member".into(),
                password: "owned member password".into(),
            },
        )
        .await
        .unwrap();
    library
        .hito
        .identity
        .set_library_access(&library.actor, &shared.id, &user.id, true)
        .await
        .unwrap();
    let session = library
        .hito
        .identity
        .login(Credentials {
            username: "owned member".into(),
            password: "owned member password".into(),
        })
        .await
        .unwrap();
    let actor = library
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let key = Key {
        scope: uuid::Uuid::new_v4(),
        selection: 1,
        entry: uuid::Uuid::new_v4(),
        start: None,
    };
    let id = FinalizationId {
        key,
        demand: uuid::Uuid::new_v4(),
        play_revision: 1,
        seek_id: None,
        restoration_id: None,
    };
    let mut native = Native::new(library.hito.catalog.clone(), library.hito.playback.clone());
    native.target = Some(Target {
        key,
        actor: actor.clone(),
        recording: uuid::Uuid::parse_str(&recording.id).unwrap(),
        playing: true,
        play_revision: 1,
        seek: None,
        restoration: None,
        output: true,
        volume: 1000,
        speed: 1000,
        permitted: true,
        next: None,
    });
    native.deadline_pending = Some(id);
    let catalog = library.hito.catalog.clone();
    let recording_id = uuid::Uuid::parse_str(&recording.id).unwrap();
    let (entered, entered_rx) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    native.spawn_finalization(id, async { true }, async move {
        let _ = entered.send(());
        let _ = release_rx.await;
        catalog
            .recordings_by_ids(&actor, &[recording_id])
            .await
            .map(|_| ())
            .map_err(PlayerError::from)
    });
    entered_rx.await.unwrap();
    library
        .hito
        .identity
        .set_library_access(&library.actor, &shared.id, &user.id, false)
        .await
        .unwrap();
    release.send(()).unwrap();
    let Some(FinalizationTaskResult::Finished(result)) = native.finalizing.join_next().await else {
        panic!("the finalizer must complete with fresh library access");
    };
    let update = native.finish_finalization(result).unwrap();
    assert_eq!(update.error, Some(PlayerError::NotFound));
    assert!(!update.exhausted);
    assert!(
        native
            .target
            .as_ref()
            .is_some_and(|target| target.key == key)
    );
    native.close().await;
}

#[tokio::test]
async fn finalizer_panic_is_fenced_and_close_drains_pending_tasks() {
    let library = hang_guard("owned finalizer panic fixture", AudioLibrary::start()).await;
    let recording = uuid::Uuid::parse_str(&library.source.recording_id).unwrap();
    let old_key = Key {
        scope: uuid::Uuid::new_v4(),
        selection: 1,
        entry: uuid::Uuid::new_v4(),
        start: None,
    };
    let current_key = Key {
        selection: 2,
        ..old_key
    };
    let old = FinalizationId {
        key: old_key,
        demand: uuid::Uuid::new_v4(),
        play_revision: 1,
        seek_id: None,
        restoration_id: None,
    };
    let current = FinalizationId {
        key: current_key,
        demand: uuid::Uuid::new_v4(),
        ..old
    };
    let mut native = Native::new(library.hito.catalog.clone(), library.hito.playback.clone());
    native.target = Some(Target {
        key: current_key,
        actor: library.actor.clone(),
        recording,
        playing: true,
        play_revision: 1,
        seek: None,
        restoration: None,
        output: true,
        volume: 1000,
        speed: 1000,
        permitted: true,
        next: None,
    });
    native.deadline_pending = Some(current);

    let (release_old, wait_old) = oneshot::channel::<()>();
    native.spawn_finalization(
        old,
        async move {
            if wait_old.await.is_ok() {
                panic!("owned retired finalizer panic");
            }
            true
        },
        async { Ok(()) },
    );
    release_old.send(()).unwrap();
    let Some(FinalizationTaskResult::Panicked(Some(failed_old))) =
        native.finalizing.join_next().await
    else {
        panic!("JoinSet must report the retired panic identity");
    };
    assert!(native.finish_finalization_panic(Some(failed_old)).is_none());
    assert!(native.deadline_pending == Some(current));
    assert!(!native.failed);
    assert!(
        native
            .target
            .as_ref()
            .is_some_and(|target| target.key == current_key)
    );

    let (release_current, wait_current) = oneshot::channel::<()>();
    native.spawn_finalization(
        current,
        async move {
            if wait_current.await.is_ok() {
                panic!("owned current finalizer panic");
            }
            true
        },
        async { Ok(()) },
    );
    release_current.send(()).unwrap();
    let Some(FinalizationTaskResult::Panicked(Some(failed_current))) =
        native.finalizing.join_next().await
    else {
        panic!("JoinSet must report the current panic identity");
    };
    let update = native
        .finish_finalization_panic(Some(failed_current))
        .unwrap();
    assert_eq!(update.phase, PlayerPhase::Failed);
    assert_eq!(update.error, Some(PlayerError::Playback));
    assert!(!update.exhausted);
    native.target = None;
    native.deadline_pending = Some(current);
    assert!(native.finish_finalization_panic(Some(current)).is_none());

    let (entered, entered_rx) = oneshot::channel();
    let (release, wait) = oneshot::channel();
    native.spawn_finalization(
        current,
        async move {
            let _ = entered.send(());
            wait.await.is_ok()
        },
        async { Ok(()) },
    );
    entered_rx.await.unwrap();
    let mut closing = Box::pin(native.close());
    let close_poll =
        std::future::poll_fn(|context| std::task::Poll::Ready(closing.as_mut().poll(context)))
            .await;
    assert!(
        close_poll.is_pending(),
        "close must wait for the held finalizer"
    );
    release.send(()).unwrap();
    closing.await;
}

#[tokio::test]
async fn unavailable_finalization_suspends_authority_without_exhausting_or_advancing() {
    let library = hang_guard("owned unavailable finalizer fixture", AudioLibrary::start()).await;
    let recording = uuid::Uuid::parse_str(&library.source.recording_id).unwrap();
    let mut owned = crate::player::attachment::Owned::new(library.actor.clone());
    owned.retrying = Some(crate::PlayerRetry { attempt: 2, of: 3 });
    owned
        .queue
        .apply(crate::PlayerAction::Replace {
            recordings: vec![recording],
            start: 0,
        })
        .unwrap();
    owned.desired_playing = true;
    let key = owned.key().unwrap();
    let mut publication = owned.published.subscribe();
    let mut state = crate::player::ownership::State {
        epoch: uuid::Uuid::new_v4(),
        owner: Some(owned),
        native: true,
    };
    let id = FinalizationId {
        key,
        demand: uuid::Uuid::new_v4(),
        play_revision: 1,
        seek_id: None,
        restoration_id: None,
    };
    let mut native = Native::new(library.hito.catalog.clone(), library.hito.playback.clone());
    native.target = Some(Target {
        key,
        actor: library.actor.clone(),
        recording,
        playing: true,
        play_revision: 1,
        seek: None,
        restoration: None,
        output: true,
        volume: 1000,
        speed: 1000,
        permitted: true,
        next: None,
    });
    native.deadline_pending = Some(id);
    native.spawn_finalization(id, async { true }, async { Err(PlayerError::Unavailable) });
    let Some(FinalizationTaskResult::Finished(result)) = native.finalizing.join_next().await else {
        panic!("the unavailable finalizer must complete with its typed access result");
    };
    let update = native.finish_finalization(result).unwrap();
    assert_eq!(update.error, Some(PlayerError::Unavailable));
    assert!(!update.exhausted);
    state.update(update);
    let published = publication.borrow_and_update().clone();
    assert_eq!(published.phase, PlayerPhase::Suspended);
    assert_eq!(published.error, Some(PlayerError::Unavailable));
    assert!(published.retrying.is_none());
    assert!(!published.playing);
    assert!(published.last_failure.is_none());
    assert_eq!(published.selection, key.selection);
    assert_eq!(published.current_index, Some(0));
    assert_eq!(
        published.current.as_ref().map(|entry| entry.recording_id),
        Some(recording)
    );
    native.close().await;
}
