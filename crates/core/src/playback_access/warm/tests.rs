use crate::{
    AudioRequest, BeginPlaybackLease, OpenedAudio,
    test_support::{hang_guard::hang_guard, provider_audio::AudioLibrary},
};
use std::time::Duration;

#[tokio::test]
async fn closing_playback_rejects_a_retained_direct_prefix() {
    let f = hang_guard("owned provider audio library setup", AudioLibrary::start()).await;
    hang_guard("owned provider audio response setup", f.ready()).await;
    let held_tail = f.server.hold_audio_tail(128);
    let mut lease_id = None;
    let mut entry = None;
    let mut retained_prefix = false;
    let mut body = None;
    let mut post_close_read_completed = false;
    let mut emitted_after_close = false;

    if let Ok(Ok(lease)) = tokio::time::timeout(
        Duration::from_secs(5),
        f.hito.playback.begin_lease(
            &f.actor,
            BeginPlaybackLease {
                recording_id: f.source.recording_id.clone(),
                source_id: f.source.id.clone(),
                client: None,
                request_id: None,
            },
        ),
    )
    .await
    {
        lease_id = Some(lease.id);
        entry = f
            .hito
            .playback
            .warm
            .state
            .lock()
            .ok()
            .and_then(|state| state.entries.get(&lease.id).cloned());
        if let Some(entry) = &entry {
            retained_prefix = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let changed = entry.changed.notified();
                    tokio::pin!(changed);
                    if entry
                        .ready
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(|ready| ready.prefix.is_some())
                    {
                        break true;
                    }
                    changed.await;
                }
            })
            .await
            .unwrap_or(false);
        }
        if retained_prefix
            && let Ok(Ok(OpenedAudio::Content(mut audio))) = tokio::time::timeout(
                Duration::from_secs(5),
                f.hito
                    .playback
                    .open_audio(&f.actor, &f.source.id, AudioRequest::default()),
            )
            .await
        {
            body = audio.body.take();
        }
    }

    let body_opened_before_close = body.is_some();
    let playback_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.playback.close()).await,
        Ok(Ok(()))
    );
    if playback_closed && let Some(body) = &mut body {
        match tokio::time::timeout(Duration::from_secs(2), body.chunk()).await {
            Ok(Ok(Some(_))) => {
                post_close_read_completed = true;
                emitted_after_close = true;
            }
            Ok(Ok(None)) | Ok(Err(_)) => post_close_read_completed = true,
            Err(_) => {}
        }
    }

    held_tail.release();
    drop(body);
    if let Some(id) = lease_id {
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            f.hito.playback.release_lease(&f.actor, id),
        )
        .await;
    }
    let replicas_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.replicas.close()).await,
        Ok(Ok(()))
    );
    let replication_closed = matches!(
        tokio::time::timeout(Duration::from_secs(5), f.hito.catalog_replication.close()).await,
        Ok(Ok(()))
    );
    let server_closed = tokio::time::timeout(Duration::from_secs(5), f.server.shutdown())
        .await
        .is_ok();

    assert!(
        playback_closed && replicas_closed && replication_closed && server_closed,
        "owned provider audio services must close within their teardown deadlines"
    );
    assert!(
        retained_prefix,
        "the owned warm entry must retain its prefix"
    );
    assert!(entry.is_some());
    assert!(
        body_opened_before_close,
        "public open_audio must return a body before playback closes"
    );
    assert!(
        post_close_read_completed,
        "the bounded body read after close must finish with an error or EOF"
    );
    assert!(
        !emitted_after_close,
        "closed playback must not emit a retained prefix"
    );
}
