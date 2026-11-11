use crate::{AudioRequest, BeginPlaybackLease, Error, test_support::playback::LocalPlayback};

#[tokio::test]
async fn playback_leases_require_foreground_sessions_without_changing_background_authority() {
    let f = LocalPlayback::start().await;
    let request = || BeginPlaybackLease {
        recording_id: f.recording.clone(),
        source_id: f.source.clone(),
    };
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, request())
        .await
        .unwrap();
    let mut background = f.actor.clone();
    background.background = true;
    assert!(
        f.services
            .playback
            .playback_source(&background, &f.recording, &f.source)
            .await
            .is_ok(),
        "ordinary background grant policy stays unchanged"
    );
    for actor in [background.clone(), {
        let mut actor = background;
        actor.unattended = true;
        actor.session_hash.clear();
        actor
    }] {
        assert!(matches!(
            f.services.playback.begin_lease(&actor, request()).await,
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            f.services.playback.lease_status(&actor, lease.id).await,
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            f.services
                .playback
                .open_lease_audio(&actor, lease.id, AudioRequest::default())
                .await,
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            f.services.playback.release_lease(&actor, lease.id).await,
            Err(Error::Forbidden)
        ));
    }
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_ok()
    );
    f.services.playback.close().await.unwrap();
    f.services.replicas.close().await.unwrap();
    f.services.catalog_replication.close().await.unwrap();
}
