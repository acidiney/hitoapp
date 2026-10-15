#[path = "fixtures/raw_audio.rs"]
mod fixture;
mod support;
use hito_core::{AdoptProviderRecording, InstallPlugin, OfferPluginService, OpenedAudio};

#[tokio::test]
async fn raw_chunked_audio_rejects_unsupported_and_ambiguous_transfer_framing() {
    for (headers, valid) in [
        ("Transfer-Encoding: chunked\r\n", true),
        ("Transfer-Encoding: gzip, chunked\r\n", false),
        ("Transfer-Encoding: chunked, chunked\r\n", false),
        ("Transfer-Encoding: chunked\r\nContent-Length: 4\r\n", false),
    ] {
        let (_directory, hito, actor, _library, music) =
            support::library("raw-audio-framing").await;
        let bytes = support::write_wav(&music.join("original.wav"), 10);
        let server = fixture::RawAudio::start(headers, &bytes).await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Raw fixture".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    service_id: service.id,
                    service_revision: 1,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let recording = hito
            .catalog
            .adopt_provider(
                &actor,
                &installation.id,
                AdoptProviderRecording {
                    installation_revision: 1,
                    provider_recording_id: "recording".into(),
                },
            )
            .await
            .unwrap();
        let source = hito
            .catalog
            .sources(&actor, &recording.id)
            .await
            .unwrap()
            .remove(0);
        let opened = hito
            .catalog
            .open_audio(&actor, &source.id, Default::default())
            .await;
        if !valid {
            assert!(
                matches!(opened, Err(hito_core::Error::Upstream(_))),
                "unsupported or ambiguous transfer framing must fail"
            );
            continue;
        }
        let OpenedAudio::Content(mut audio) = opened.unwrap() else {
            panic!("ordinary chunked audio must open")
        };
        let mut received = Vec::new();
        while let Some(bytes) = audio.body.as_mut().unwrap().chunk().await.unwrap() {
            received.extend_from_slice(&bytes);
        }
        assert_eq!(received, bytes);
    }
}
