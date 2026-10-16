use crate::{Config, router};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hito_plugin_sdk::SearchPage;
use http_body_util::BodyExt;
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tower::ServiceExt;

#[tokio::test]
async fn successful_search_terminates_owned_descendants_with_closed_standard_streams() {
    let directory = tempfile::tempdir().unwrap();
    let _cleanup = ReleaseOnDrop(directory.path().to_owned());
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/success_child.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        router(Config { executable }).oneshot(
            Request::post("/v1/search")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"query":"Owned tone","cursor":null,"limit":1}).to_string(),
                ))
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let page: SearchPage = serde_json::from_slice(&bytes).unwrap();
    page.validate(1).unwrap();
    assert_eq!(page.items[0].id, "HitoTone001");

    assert!(directory.path().join("child.ready").exists());
    let descendant = ProcessIdentity::read(&directory.path().join("child.identity")).unwrap();
    let pid: u32 = fs::read_to_string(directory.path().join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(descendant.pid, pid);
    let leader: u32 = fs::read_to_string(directory.path().join("leader.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(descendant.group, leader);

    let deadline = Instant::now() + Duration::from_secs(1);
    while descendant.running().unwrap() {
        assert!(
            Instant::now() < deadline,
            "successful search left an owned descendant running"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

struct ProcessIdentity {
    pid: u32,
    group: u32,
    started: u64,
    state: char,
}

impl ProcessIdentity {
    fn read(path: &Path) -> io::Result<Self> {
        let value = fs::read_to_string(path)?;
        let invalid = || io::Error::other("Invalid owned fixture process identity");
        let pid = value
            .split_once(' ')
            .ok_or_else(invalid)?
            .0
            .parse()
            .map_err(|_| invalid())?;
        let fields: Vec<_> = value
            .rsplit_once(") ")
            .ok_or_else(invalid)?
            .1
            .split_whitespace()
            .collect();
        Ok(Self {
            pid,
            group: fields
                .get(2)
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
            started: fields
                .get(19)
                .ok_or_else(invalid)?
                .parse()
                .map_err(|_| invalid())?,
            state: fields
                .first()
                .and_then(|field| field.chars().next())
                .ok_or_else(invalid)?,
        })
    }

    fn running(&self) -> io::Result<bool> {
        match Self::read(Path::new(&format!("/proc/{}/stat", self.pid))) {
            Ok(current) => Ok(current.pid == self.pid
                && current.started == self.started
                && !matches!(current.state, 'Z' | 'X')),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

struct ReleaseOnDrop(PathBuf);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if fs::write(self.0.join("release"), []).is_err() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(descendant) = ProcessIdentity::read(&self.0.join("child.identity"))
                && matches!(descendant.running(), Ok(false))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
