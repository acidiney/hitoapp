use super::{
    durable::reopen,
    fixture::{RemoteServer, local},
};
use hito_core::{Actor, Credentials, DeleteAccount, Services, User};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

struct Member {
    user: User,
    actor: Actor,
    binding: Uuid,
    path: PathBuf,
}

async fn member(
    directory: &Path,
    services: &Services,
    admin: &Actor,
    remote: &RemoteServer,
    name: &str,
) -> Member {
    let credentials = || Credentials {
        username: name.into(),
        password: "owned retirement fixture password".into(),
    };
    let user = services
        .identity
        .create_user(admin, credentials())
        .await
        .unwrap();
    let login = services.identity.login(credentials()).await.unwrap();
    let actor = services.identity.authenticate(&login.token).await.unwrap();
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    Member {
        user,
        actor,
        binding: connection.id,
        path: directory
            .join("hito.replicas")
            .join(connection.id.to_string()),
    }
}

async fn erase(services: &Services, admin: &Actor, member: &Member) {
    services
        .identity
        .delete_account(
            admin,
            &member.user.id,
            DeleteAccount {
                username: member.user.username.clone(),
            },
        )
        .await
        .unwrap();
}

async fn pending(directory: &Path) -> i64 {
    use sqlx::Connection;
    let mut database = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(directory.join("hito.db")),
    )
    .await
    .unwrap();
    let count = sqlx::query_scalar("SELECT count(*) FROM erased_replica_profiles")
        .fetch_one(&mut database)
        .await
        .unwrap();
    database.close().await.unwrap();
    count
}

fn data_family(path: &Path) {
    for name in ["hito.db-wal", "hito.db-shm", "hito.db-journal"] {
        std::fs::copy(path.join("hito.db"), path.join(name)).unwrap();
    }
}

#[tokio::test]
async fn retirement_resumes_every_supported_data_and_metadata_crash_boundary() {
    let (directory, services, admin) = local().await;
    let remote = RemoteServer::start().await;
    let kept = services
        .replicas
        .connect(&admin, remote.request())
        .await
        .unwrap();
    let kept_path = directory
        .path()
        .join("hito.replicas")
        .join(kept.id.to_string());
    let mut profiles = Vec::new();
    for stage in 0..8 {
        let profile = member(
            directory.path(),
            &services,
            &admin,
            &remote,
            &format!("retirement-stage-{stage}"),
        )
        .await;
        std::fs::write(
            profile.path.join("user-original"),
            b"preserve the user's original",
        )
        .unwrap();
        erase(&services, &admin, &profile).await;
        profiles.push(profile);
    }
    services.replicas.close().await.unwrap();
    drop(services);
    for (stage, profile) in profiles.iter().enumerate() {
        assert_eq!(
            std::fs::read(profile.path.join("user-original")).unwrap(),
            b"preserve the user's original"
        );
        std::fs::remove_file(profile.path.join("user-original")).unwrap();
        data_family(&profile.path);
        for name in ["hito.db", "hito.db-wal", "hito.db-shm", "hito.db-journal"]
            .iter()
            .take(stage.min(4))
        {
            std::fs::remove_file(profile.path.join(name)).unwrap();
        }
        if stage >= 5 {
            std::fs::remove_file(profile.path.join("owner")).unwrap();
        }
        if stage >= 6 {
            std::fs::remove_file(profile.path.join("profile.lock")).unwrap();
        }
        if stage == 7 {
            std::fs::remove_dir(&profile.path).unwrap();
        }
    }
    let (services, _) = reopen(directory.path()).await;
    for profile in &profiles {
        assert!(
            !profile.path.exists(),
            "retirement did not finish a supported crash boundary"
        );
    }
    assert_eq!(pending(directory.path()).await, 0);
    assert!(kept_path.join("hito.db").is_file());
    services.replicas.close().await.unwrap();
    drop(services);
    let (services, _) = reopen(directory.path()).await;
    assert_eq!(pending(directory.path()).await, 0);
    assert!(kept_path.join("hito.db").is_file());
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn markerless_data_is_never_adopted_as_a_completed_retirement() {
    let (directory, services, admin) = local().await;
    let remote = RemoteServer::start().await;
    let profile = member(
        directory.path(),
        &services,
        &admin,
        &remote,
        "markerless-profile",
    )
    .await;
    std::fs::write(
        profile.path.join("user-original"),
        b"preserve this original",
    )
    .unwrap();
    erase(&services, &admin, &profile).await;
    services.replicas.close().await.unwrap();
    drop(services);
    let expected = std::fs::read(profile.path.join("hito.db")).unwrap();
    std::fs::remove_file(profile.path.join("user-original")).unwrap();
    std::fs::remove_file(profile.path.join("owner")).unwrap();
    let (services, _) = reopen(directory.path()).await;
    assert_eq!(
        std::fs::read(profile.path.join("hito.db")).unwrap(),
        expected
    );
    assert!(!profile.path.join("owner").exists());
    assert_eq!(pending(directory.path()).await, 1);
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn an_earlier_failed_retirement_does_not_keep_another_accounts_request_or_files() {
    for locked in [false, true] {
        let (directory, services, admin) = local().await;
        let remote = RemoteServer::start().await;
        let kept = services
            .replicas
            .connect(&admin, remote.request())
            .await
            .unwrap();
        let a = member(
            directory.path(),
            &services,
            &admin,
            &remote,
            "first-retirement-account",
        )
        .await;
        let b = member(
            directory.path(),
            &services,
            &admin,
            &remote,
            "second-retirement-account",
        )
        .await;
        let (first, second) = if a.binding < b.binding {
            (a, b)
        } else {
            (b, a)
        };
        std::fs::write(first.path.join("user-original"), b"must remain untouched").unwrap();
        erase(&services, &admin, &first).await;
        let blocker = if locked {
            std::fs::remove_file(first.path.join("user-original")).unwrap();
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(first.path.join("profile.lock"))
                .unwrap();
            file.try_lock().unwrap();
            Some(file)
        } else {
            None
        };
        let session = services
            .replicas
            .activate(&second.actor, second.binding)
            .await
            .unwrap();
        remote.hold_next_pull();
        let replicas = services.replicas.clone();
        let actor = second.actor.clone();
        let mut request = tokio::spawn(async move { replicas.synchronize(&actor, &session).await });
        remote.wait_for_pull().await;
        erase(&services, &admin, &second).await;
        let cancelled = tokio::time::timeout(Duration::from_secs(1), &mut request).await;
        remote.release_pull();
        if cancelled.is_err() {
            let _ = request.await;
        }
        assert!(
            cancelled.is_ok(),
            "an earlier failed UUID kept a later account's request alive"
        );
        assert!(cancelled.unwrap().unwrap().is_err());
        assert!(
            !second.path.exists(),
            "an earlier failed UUID prevented later file retirement"
        );
        assert!(first.path.join("hito.db").is_file());
        if !locked {
            assert_eq!(
                std::fs::read(first.path.join("user-original")).unwrap(),
                b"must remain untouched"
            );
        }
        assert_eq!(pending(directory.path()).await, 1);
        let kept_path = directory
            .path()
            .join("hito.replicas")
            .join(kept.id.to_string());
        assert!(kept_path.join("hito.db").is_file());
        drop(blocker);
        services.replicas.close().await.unwrap();
        drop(services);
        if !locked {
            std::fs::remove_file(first.path.join("user-original")).unwrap();
        }
        let (services, _) = reopen(directory.path()).await;
        assert_eq!(pending(directory.path()).await, 0);
        assert!(!first.path.exists());
        assert!(kept_path.join("hito.db").is_file());
        services.replicas.close().await.unwrap();
    }
}
