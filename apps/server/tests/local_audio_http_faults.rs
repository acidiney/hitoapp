#![cfg(target_os = "linux")]

use hito_testkit::read_fault_shim as shim;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use hito_core::test_support::hang_guard::hang_guard;
use http_body_util::BodyExt;
use std::time::Duration;
use tower::ServiceExt;

struct Listener {
    app: Router,
    token: String,
    source: String,
}

impl Listener {
    async fn get(&self) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/sources/{}/audio", self.source))
                    .header("x-hito-request", "1")
                    .header("authorization", format!("Bearer {}", self.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn whole(&self) -> Vec<u8> {
        let response = self.get().await;
        assert_eq!(response.status(), StatusCode::OK);
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }
}

fn write_long_wav(path: &std::path::Path) -> Vec<u8> {
    let samples = 100_000u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&8000u32.to_le_bytes());
    bytes.extend_from_slice(&16000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for index in 0..samples {
        let sample = ((index % 200) as i16 - 100) * 100;
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, &bytes).unwrap();
    bytes
}

#[tokio::test]
async fn local_audio_over_http_survives_a_stalled_open_and_a_drive_lost_mid_stream() {
    let test = "local_audio_over_http_survives_a_stalled_open_and_a_drive_lost_mid_stream";
    let Some(faults) = shim::child_faults() else {
        shim::run_in_child(test).await;
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let music = directory.path().join("hito-faulty-drive");
    std::fs::create_dir(&music).unwrap();
    let original = write_long_wav(&music.join("track.wav"));
    let services = hang_guard(
        "opening the library",
        hito_core::open(hito_core::CoreConfig::new(
            directory.path().join("hito.db"),
            music.clone(),
            hito_core::HostDeviceId::new("api-test-host").unwrap(),
        )),
    )
    .await
    .unwrap();
    let session = services
        .identity
        .setup(hito_core::Credentials {
            username: "admin".into(),
            password: "local audio http fault password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let library = services
        .catalog
        .libraries(&actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    hito_core::test_support::support::scan(&services, &actor, &library).await;
    let recording = services
        .catalog
        .recordings(&actor, hito_core::Search::default())
        .await
        .unwrap()
        .items
        .remove(0)
        .id;
    let source = services
        .catalog
        .sources(&actor, &recording)
        .await
        .unwrap()
        .remove(0)
        .id;
    let listener = Listener {
        app: hito_server::router(services),
        token: session.token,
        source,
    };
    assert_eq!(hang_guard("playback", listener.whole()).await, original);

    faults.set("stall-open");
    let stalled = hang_guard("stalled open", listener.get()).await;
    assert_eq!(stalled.status(), StatusCode::NOT_FOUND);
    let body = stalled.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains("hito-faulty"), "{text}");
    faults.clear("stall-open");
    hang_guard("recovered open", async {
        loop {
            let response = listener.get().await;
            if response.status() == StatusCode::OK {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;

    let response = listener.get().await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut received = 0;
    let first = hang_guard("first frame", body.frame())
        .await
        .unwrap()
        .unwrap();
    received += first.into_data().unwrap().len();
    faults.set("fail-read");
    let outcome = hang_guard("frames after the drive failed", async {
        loop {
            match body.frame().await {
                Some(Ok(frame)) => received += frame.into_data().map_or(0, |data| data.len()),
                Some(Err(_)) => return "aborted",
                None => return "ended",
            }
        }
    })
    .await;
    assert_eq!(outcome, "aborted");
    assert!(received < original.len());

    let unplugged = directory.path().join("hito-faulty-unplugged");
    std::fs::rename(&music, &unplugged).unwrap();
    let missing = hang_guard("unplugged request", listener.get()).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    std::fs::rename(&unplugged, &music).unwrap();
    faults.clear("fail-read");
    assert_eq!(
        hang_guard("recovered playback", listener.whole()).await,
        original
    );
}
