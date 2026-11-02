use crate::{Config, Service, router};
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

const HANG_GUARD: Duration = Duration::from_secs(30);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_terminates_and_reaps_owned_work_before_rejecting_new_searches() {
    let directory = tempfile::tempdir().unwrap();
    let _cleanup = HeldReleaseOnDrop(directory.path().to_owned());
    let executable = directory.path().join("owned-yt-dlp");
    fs::write(&executable, include_str!("../fixtures/shutdown.sh")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::new(Config { executable });
    let request = || {
        Request::post("/v1/search")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"query":"Owned held tone","cursor":null,"limit":1}).to_string(),
            ))
            .unwrap()
    };
    let first = tokio::spawn(service.router().oneshot(request()));
    let second = tokio::spawn(service.router().oneshot(request()));
    let hang_deadline = Instant::now() + HANG_GUARD;
    while !["first", "second"]
        .iter()
        .all(|slot| directory.path().join(slot).join("leader.ready").exists())
    {
        assert!(
            !first.is_finished() && !second.is_finished(),
            "An owned extractor ended before both became ready"
        );
        assert!(
            Instant::now() < hang_deadline,
            "Both owned extractors must become ready"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut operations = Vec::new();
    for slot in ["first", "second"] {
        let operation = directory.path().join(slot);
        let leader = ProcessIdentity::read(&operation.join("leader.identity")).unwrap();
        let descendant = ProcessIdentity::read(&operation.join("child.identity")).unwrap();
        assert_eq!(descendant.group, leader.pid);
        assert_eq!(leader.group, leader.pid);
        assert!(leader.running().unwrap());
        assert!(descendant.running().unwrap());
        operations.push((leader, descendant));
    }
    assert!(!first.is_finished());
    assert!(!second.is_finished());
    assert_eq!(
        fs::read_to_string(directory.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        2
    );

    tokio::time::timeout(HANG_GUARD, service.shutdown())
        .await
        .unwrap()
        .unwrap();
    let stopped_at_return = operations
        .iter()
        .all(|(leader, descendant)| !leader.present().unwrap() && !descendant.running().unwrap());
    if !stopped_at_return {
        fs::write(directory.path().join("release"), []).unwrap();
    }
    let mut statuses = Vec::new();
    for pending in [first, second] {
        let response = tokio::time::timeout(HANG_GUARD, pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        statuses.push((status, bytes));
    }
    let response = tokio::time::timeout(HANG_GUARD, service.router().oneshot(request()))
        .await
        .unwrap()
        .unwrap();
    let rejected = response.status();
    let calls = fs::read_to_string(directory.path().join("calls"))
        .unwrap()
        .lines()
        .count();

    assert!(
        stopped_at_return,
        "Shutdown returned before owned processes terminated and direct children were reaped"
    );
    for (status, bytes) in statuses {
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"error":"The YouTube service is unavailable"})
        );
    }
    assert_eq!(rejected, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(calls, 2, "Shutdown admitted another executable invocation");
}

pub(super) struct ProcessIdentity {
    pub(super) pid: u32,
    pub(super) group: u32,
    started: u64,
    state: char,
}

impl ProcessIdentity {
    pub(super) fn read(path: &Path) -> io::Result<Self> {
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

    pub(super) fn running(&self) -> io::Result<bool> {
        match Self::read(Path::new(&format!("/proc/{}/stat", self.pid))) {
            Ok(current) => Ok(current.pid == self.pid
                && current.started == self.started
                && !matches!(current.state, 'Z' | 'X')),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) fn present(&self) -> io::Result<bool> {
        match Self::read(Path::new(&format!("/proc/{}/stat", self.pid))) {
            Ok(current) => Ok(current.pid == self.pid && current.started == self.started),
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

pub(super) struct HeldReleaseOnDrop(pub(super) PathBuf);

impl Drop for HeldReleaseOnDrop {
    fn drop(&mut self) {
        if fs::write(self.0.join("release"), []).is_err() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let stopped = ["first", "second"].iter().all(|slot| {
                let directory = self.0.join(slot);
                let Ok(leader) = ProcessIdentity::read(&directory.join("leader.identity")) else {
                    return false;
                };
                let Ok(descendant) = ProcessIdentity::read(&directory.join("child.identity"))
                else {
                    return false;
                };
                matches!(leader.present(), Ok(false)) && matches!(descendant.running(), Ok(false))
            });
            if stopped {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
