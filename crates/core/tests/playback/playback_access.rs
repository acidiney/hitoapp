use fixture::LocalPlayback;
use hito_core::test_support::playback as fixture;
use hito_core::test_support::support;

#[tokio::test]
async fn logout_revokes_an_opened_playback_grant_and_a_retained_actor_cannot_reopen_it() {
    let f = LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    assert_eq!(
        support::audio_bytes(grant.open_file().await.unwrap()),
        f.original
    );
    f.services.identity.logout(&f.token).await.unwrap();
    assert!(grant.check().await.is_err());
    assert!(grant.open_file().await.is_err());
    assert!(
        f.services
            .playback
            .playback_source(&f.actor, &f.recording, &f.source)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn restoring_library_access_does_not_revive_a_previous_playback_grant() {
    let f = LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    let other = hito_core::open(hito_core::CoreConfig::new(
        f.directory.path().join("hito.db"),
        f.music.clone(),
        hito_core::HostDeviceId::new("playback").unwrap(),
    ))
    .await
    .unwrap();
    other
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, false)
        .await
        .unwrap();
    other
        .identity
        .set_library_access(&f.actor, &f.library, &f.actor.user().id, true)
        .await
        .unwrap();
    assert!(
        grant.check().await.is_err(),
        "restoring permission cannot revive old output"
    );
    assert!(
        f.services
            .playback
            .playback_source(&f.actor, &f.recording, &f.source)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn playback_authority_requires_the_recording_source_pair_and_the_accounts_membership() {
    let f = LocalPlayback::start().await;
    f.services
        .identity
        .create_user(
            &f.actor,
            hito_core::Credentials {
                username: "listener".into(),
                password: "private listener password".into(),
            },
        )
        .await
        .unwrap();
    let session = f
        .services
        .identity
        .login(hito_core::Credentials {
            username: "listener".into(),
            password: "private listener password".into(),
        })
        .await
        .unwrap();
    let other = f
        .services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    assert!(matches!(
        f.services
            .playback
            .playback_source(&other, &f.recording, &f.source)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    assert!(matches!(
        f.services
            .playback
            .playback_source(&f.actor, &uuid::Uuid::new_v4().to_string(), &f.source)
            .await,
        Err(hito_core::Error::NotFound)
    ));
    let debug = format!("{:?}", f.actor);
    use sha2::Digest;
    assert!(!debug.contains(&f.token));
    assert!(!debug.contains(&format!("{:x}", sha2::Sha256::digest(f.token.as_bytes()))));
}

#[tokio::test]
async fn expired_sessions_notify_the_owner_without_a_new_command_or_read() {
    let f = LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    let waiting = tokio::spawn(async move { grant.revoked().await });
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(f.directory.path().join("hito.db")),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE sessions SET expires_at = unixepoch() - 1 WHERE user_id = ?")
        .bind(&f.actor.user().id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .expect("an idle owner must learn about expiry")
            .unwrap(),
        hito_core::Error::Unauthorized
    ));
    pool.close().await;
}

#[tokio::test]
async fn reconnecting_a_folder_and_rescanning_cannot_restore_old_playback_authority() {
    let f = LocalPlayback::start().await;
    let grant = f
        .services
        .playback
        .playback_source(&f.actor, &f.recording, &f.source)
        .await
        .unwrap();
    support::scan(&f.services, &f.actor, &f.library).await;
    grant
        .check()
        .await
        .expect("an ordinary rescan preserves output authority");
    let folder = f
        .services
        .storage
        .folders(&f.actor, &f.library)
        .await
        .unwrap()
        .remove(0);
    f.services
        .storage
        .remove_folder(&f.actor, &folder.id, folder.revision)
        .await
        .unwrap();
    f.services
        .storage
        .connect_folder(
            &f.actor,
            &f.library,
            hito_core::ConnectFolder {
                directory: folder.directory.unwrap(),
                name: None,
            },
        )
        .await
        .unwrap();
    assert!(f.services.storage.run_next_job().await.unwrap());
    assert!(grant.check().await.is_err());
    assert!(
        f.services
            .playback
            .playback_source(&f.actor, &f.recording, &f.source)
            .await
            .is_ok()
    );
}
