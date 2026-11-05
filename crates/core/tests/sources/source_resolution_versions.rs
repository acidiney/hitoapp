use hito_core::test_support::hang_guard::HANG_CEILING;
#[path = "../fixtures/provider_attachment.rs"]
#[allow(dead_code, unused_imports)]
mod fixture;
use hito_core::test_support::support;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    Actor, AdoptProviderRecording, OpenedAudio, PluginInstallation, ResolveSource, Services,
    SourceResolution, SourceResolutionOutcome,
};
use serde_json::json;
use std::panic::AssertUnwindSafe;
use uuid::Uuid;

#[tokio::test]
async fn automatic_matching_distinguishes_a_live_album_from_a_song_named_live() {
    let (directory, hito, actor, _, music) = support::library("source-version-matching").await;
    let original = support::write_wav(&music.join("owned-version-control.wav"), 37);
    let server = fixture::ProviderServer::start().await;
    let result = tokio::time::timeout(
        HANG_CEILING,
        AssertUnwindSafe(async {
            let installed = fixture::connect(
                &hito,
                &actor,
                &server,
                "org.hito.version-fixture",
                &["search", "sources"],
            )
            .await;
            server
                .audio(
                    StatusCode::OK,
                    HeaderMap::from_iter([(
                        axum::http::header::CONTENT_TYPE,
                        HeaderValue::from_static("audio/wav"),
                    )]),
                    original.clone(),
                )
                .await;
            let control = resolve_case(
                &hito,
                &actor,
                &server,
                &installed,
                Case {
                    key: "control",
                    title: "Live",
                    target_album: "Owned studio album",
                    candidate_album: "Owned studio album",
                },
            )
            .await;
            assert!(
                matches!(control.outcome, SourceResolutionOutcome::Ready),
                "a studio song named Live must remain eligible for automatic playback"
            );
            assert_eq!(control.sources.len(), 1);
            assert!(control.sources[0].ready);
            let opened = hito
                .playback
                .open_audio(&actor, &control.sources[0].id, Default::default())
                .await
                .unwrap_or_else(|_| panic!("the accepted control source must open"));
            let OpenedAudio::Content(mut audio) = opened else {
                panic!("the accepted control must yield owned audio")
            };
            let mut received = Vec::new();
            let body = audio
                .body
                .as_mut()
                .expect("the accepted control must supply an audio body");
            while let Some(chunk) = body
                .chunk()
                .await
                .unwrap_or_else(|_| panic!("the owned control audio must complete"))
            {
                received.extend_from_slice(&chunk);
            }
            assert!(
                received == original,
                "the control audio must preserve owned bytes"
            );
            assert_eq!(server.requests("v1/search").await.len(), 1);
            assert_eq!(server.requests("v1/recordings/detail").await.len(), 2);
            assert_eq!(server.requests("v1/sources").await.len(), 2);
            assert_eq!(server.requests("v1/audio").await.len(), 1);

            let mut outcomes = Vec::new();
            for (studio_key, live_key, title, live_album) in [
                ("studio-target", "live-target", "Owned performance", "Live"),
                ("studio-sessions-target", "live-sessions-target", "Owned performance", "Live Sessions"),
                ("studio-year-target", "live-year-target", "Owned performance", "Live 1998"),
                ("studio-live-title-target", "live-live-title-target", "Live", "Live Sessions"),
            ] {
                let studio = resolve_case(
                    &hito,
                    &actor,
                    &server,
                    &installed,
                    Case {
                        key: studio_key,
                        title,
                        target_album: "Owned studio album",
                        candidate_album: live_album,
                    },
                )
                .await;
                let live = resolve_case(
                    &hito,
                    &actor,
                    &server,
                    &installed,
                    Case {
                        key: live_key,
                        title,
                        target_album: live_album,
                        candidate_album: "Owned studio album",
                    },
                )
                .await;
                outcomes.push((studio, live));
            }
            assert_eq!(server.requests("v1/search").await.len(), 9);
            assert_eq!(server.requests("v1/audio").await.len(), 1);
            for (studio, live) in outcomes {
                assert!(
                    matches!(studio.outcome, SourceResolutionOutcome::NoSuitableSource)
                        && studio.sources.is_empty(),
                    "a studio recording must reject an otherwise exact ready candidate with declared live album context"
                );
                assert!(
                    matches!(live.outcome, SourceResolutionOutcome::NoSuitableSource)
                        && live.sources.is_empty(),
                    "a recording with declared live album context must reject an otherwise exact ready studio candidate"
                );
            }
        })
        .catch_unwind(),
    )
    .await;
    server.shutdown().await;
    drop(hito);
    drop(directory);
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => {
            panic!("the owned version-matching exercise must settle within its test deadline")
        }
    }
}

struct Case {
    key: &'static str,
    title: &'static str,
    target_album: &'static str,
    candidate_album: &'static str,
}

async fn resolve_case(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    installed: &PluginInstallation,
    case: Case,
) -> SourceResolution {
    let target = provider_recording(
        &format!("owned/target-{}", case.key),
        case.title,
        case.target_album,
    );
    server.reply("v1/recordings/detail", target.clone()).await;
    server.reply("v1/sources", json!({"items": []})).await;
    let recording = hito
        .catalog
        .adopt_provider(
            actor,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: target["id"].as_str().unwrap().into(),
            },
        )
        .await
        .unwrap_or_else(|_| panic!("the owned target recording must be adopted"));
    assert!(
        !recording.available,
        "the owned target must start without a ready source"
    );
    let selected = hito
        .catalog
        .recording_metadata(actor, &recording.id, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the target metadata must be readable"))
        .selected_observation;
    let selected = serde_json::to_value(selected).unwrap();
    let candidate = provider_recording(
        &format!("owned/candidate-{}", case.key),
        case.title,
        case.candidate_album,
    );
    server
        .reply(
            "v1/search",
            json!({"items": [candidate], "next_cursor": null}),
        )
        .await;
    server.reply("v1/recordings/detail", candidate).await;
    server
        .reply(
            "v1/sources",
            json!({"items": [{"id": format!("owned/source-{}", case.key), "label": "Owned version source", "ready": true}]}),
        )
        .await;
    let request_id = Uuid::new_v4();
    let resolution = hito
        .catalog
        .resolve_source_until(
            actor,
            &recording.id,
            ResolveSource { request_id },
            std::future::pending(),
        )
        .await
        .unwrap_or_else(|_| panic!("the owned source resolution must return a typed outcome"));
    assert_eq!(resolution.request_id, request_id);
    assert!(
        resolution.recording_id == recording.id,
        "resolution must preserve target identity"
    );
    let after = hito
        .catalog
        .recording_metadata(actor, &recording.id, Default::default())
        .await
        .unwrap_or_else(|_| panic!("the target metadata must remain readable"))
        .selected_observation;
    assert!(
        serde_json::to_value(after).unwrap() == selected,
        "automatic matching must preserve the target metadata observation"
    );
    resolution
}

fn provider_recording(id: &str, title: &str, album: &str) -> serde_json::Value {
    json!({
        "id": id,
        "title": title,
        "artist": "Owned performer",
        "duration_ms": 1000,
        "release": {"id": format!("{id}/release"), "title": album, "artist": "Owned performer"}
    })
}
