#![cfg(feature = "test-support")]

#[path = "qualification/mod.rs"]
pub mod qualification;

use hito_debrid_premiumize::{TransferBinding, TransferFileBinding};
use hito_plugin_premiumize::{Config, CuratedWavRecording, router};
use hito_plugin_sdk::{ProviderRecording, RecordingAttribution};
use hito_testkit::logs::LogCapture;
use qualification::{provider::recording, run, transfer};

const AGGREGATE_BYTES: usize = 49_152;

#[tokio::test]
async fn the_aggregate_budget_counts_recordings_only_and_rejections_log_a_safe_reason() {
    run(vec![transfer("queued", 0.0)], |case| {
        Box::pin(async move {
            let third = AGGREGATE_BYTES / 3;
            assert_eq!(third * 3, AGGREGATE_BYTES);

            let exact = entries([third, third, third]);
            let private_and_recordings = exact
                .iter()
                .map(|entry| {
                    serde_json::to_vec(&(
                        &entry.recording,
                        &entry.source_id,
                        &entry.label,
                        &entry.binding,
                    ))
                    .unwrap()
                    .len()
                })
                .sum::<usize>();
            assert!(private_and_recordings > AGGREGATE_BYTES);
            assert!(router(with(case.config(), exact)).is_ok());

            let capture = LogCapture::default();
            let rejected = tracing::dispatcher::with_default(
                &capture.dispatch("off,hito_plugin_sdk=trace"),
                || router(with(case.config(), entries([third, third, third + 1]))),
            );
            assert!(rejected.is_err());
            let events = capture.events();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["fields"]["reason"], "CatalogTooLarge");
            capture.assert_excludes(&["owned-private"]);
        })
    })
    .await;
}

fn with(mut config: Config, recordings: Vec<CuratedWavRecording>) -> Config {
    config.connections.truncate(1);
    config.connections[0].recordings = recordings;
    config
}

fn entries<const N: usize>(sizes: [usize; N]) -> Vec<CuratedWavRecording> {
    sizes
        .into_iter()
        .enumerate()
        .map(|(index, bytes)| {
            let mut value = recording(false);
            value.id = format!("curated-recording-{index}");
            CuratedWavRecording {
                recording: padded(value, bytes),
                source_id: format!("curated-source-{index}"),
                label: "Curated canonical PCM16 WAV".into(),
                binding: TransferFileBinding {
                    transfer: TransferBinding {
                        transfer_id: format!("owned-private-transfer-{index}"),
                        created_at_unix_seconds: 1_700_000_000,
                    },
                    item_id: format!("owned-private-item-{index}"),
                    item_created_at_unix_seconds: 1_700_000_120,
                    file_name: format!("owned-private-file-{index}.wav"),
                    file_bytes: 1_000,
                    folder_id: format!("owned-private-folder-{index}"),
                },
            }
        })
        .collect()
}

fn padded(mut value: ProviderRecording, bytes: usize) -> ProviderRecording {
    value.attribution = Some(Box::new(RecordingAttribution {
        creators: vec!["a".into()],
        provider_label: "owned-attribution".into(),
        content_url: "https://example.invalid/owned".into(),
    }));
    loop {
        let encoded = serde_json::to_vec(&value).unwrap().len();
        let missing = bytes.checked_sub(encoded).expect("the base recording fits");
        if missing == 0 {
            break;
        }
        let creators = &mut value.attribution.as_mut().unwrap().creators;
        match creators.iter_mut().find(|creator| creator.len() < 2048) {
            Some(creator) => creator.push_str(&"a".repeat(missing.min(2048 - creator.len()))),
            None => creators.push("a".into()),
        }
    }
    value.validate().unwrap();
    value
}
