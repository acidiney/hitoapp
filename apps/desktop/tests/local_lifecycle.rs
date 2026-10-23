use hito_desktop::{Error, LocalHost, Paths};
use reqwest::{Client, StatusCode};
use std::path::Path;

fn paths(directory: &Path) -> Paths {
    let web = directory.join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    Paths {
        data: directory.join("data"),
        music: directory.join("music"),
        web,
    }
}

#[tokio::test]
async fn one_local_host_owns_a_library_and_restart_rejects_the_previous_window_cookie() {
    let directory = tempfile::tempdir().unwrap();
    let first = LocalHost::start(paths(directory.path())).await.unwrap();
    assert!(matches!(
        LocalHost::start(paths(directory.path())).await,
        Err(Error::AlreadyRunning)
    ));
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client.get(first.entry_url()).send().await.unwrap();
    let old = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let identity = std::fs::read(directory.path().join("data/hito.device-id")).unwrap();
    first.shutdown().await.unwrap();
    let second = LocalHost::start(paths(directory.path())).await.unwrap();
    let root = second.entry_url().join("/").unwrap();
    assert_eq!(
        client
            .get(root.join("health").unwrap())
            .header("cookie", old)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = client.get(second.entry_url()).send().await.unwrap();
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        client
            .get(root.join("health").unwrap())
            .header("cookie", cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        std::fs::read(directory.path().join("data/hito.device-id")).unwrap(),
        identity
    );
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_startup_releases_the_library_claim_for_a_later_healthy_start() {
    let directory = tempfile::tempdir().unwrap();
    let mut invalid = paths(directory.path());
    invalid.web = directory.path().join("missing-assets");
    assert!(matches!(
        LocalHost::start(invalid).await,
        Err(Error::Startup)
    ));
    let host = LocalHost::start(paths(directory.path())).await.unwrap();
    host.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn local_storage_is_private_and_a_symlink_cannot_redirect_the_application_directory() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");
    let target = directory.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let permissions = std::fs::metadata(&target).unwrap().permissions().mode();
    symlink(&target, &data).unwrap();
    assert!(matches!(
        LocalHost::start(paths(directory.path())).await,
        Err(Error::Storage)
    ));
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode(),
        permissions
    );
    std::fs::remove_file(data).unwrap();
    let host = LocalHost::start(paths(directory.path())).await.unwrap();
    assert_eq!(
        std::fs::metadata(directory.path().join("data"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(directory.path().join("data/desktop.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    host.shutdown().await.unwrap();
}
