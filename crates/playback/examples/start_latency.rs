#[path = "start_latency/workflow.rs"]
mod workflow;
use workflow::{Recovery, Rig, recovery_once};
#[path = "start_latency/measurement.rs"]
mod measurement;
use measurement::{Measurements, failure, milliseconds, start_fields};
#[path = "start_latency/provider.rs"]
mod provider;
use provider::ProviderProcess;
#[path = "start_latency/capture.rs"]
mod capture;
#[path = "start_latency/process.rs"]
mod process;
use capture::Monitor;
use hito_core::{
    Actor, AdoptProviderRecording, CoreConfig, Credentials, HostDeviceId, InstallPlugin,
    OfferPluginService, Search, Services,
};
use hito_playback::Player;
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

type Failure = Box<dyn std::error::Error + Send + Sync>;

fn variable(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

async fn find(services: &Services, actor: &Actor, title: &str) -> String {
    let page = services
        .catalog
        .recordings(
            actor,
            Search {
                q: Some(title.into()),
                limit: Some(5),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    page.items
        .into_iter()
        .find(|item| item.title == title)
        .unwrap_or_else(|| panic!("{title} is missing from the library"))
        .id
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Failure> {
    let samples: usize = variable("BENCH_SAMPLES", "100").parse()?;
    let warmup: usize = variable("BENCH_WARMUP", "5").parse()?;
    let rtt: u64 = variable("BENCH_RTT_MS", "40").parse()?;
    let format_span = variable("BENCH_TRACKS", "10000").parse::<usize>()? / 3;
    let out = PathBuf::from(variable("BENCH_OUT", "/workspace/test-results/latency"));
    std::fs::create_dir_all(&out)?;
    let group = samples + warmup;

    let directory = tempfile::tempdir_in("/dev/shm")?;
    let config = CoreConfig::new(
        directory.path().join("hito.db"),
        PathBuf::from(variable("BENCH_LIBRARY", "/music/Music")),
        HostDeviceId::new("bench-native")?,
    );
    let services = hito_core::open(config).await?;
    let session = services
        .identity
        .setup(Credentials {
            username: "bench-admin".into(),
            password: "latency benchmark password".into(),
        })
        .await?;
    let actor = services.identity.authenticate(&session.token).await?;
    let library = services.catalog.libraries(&actor).await?.remove(0).id;
    let scan_started = Instant::now();
    let job = services
        .storage
        .enqueue_scan(&actor, &library)
        .await?
        .remove(0);
    assert!(services.storage.run_next_job().await?);
    let job = services.storage.job(&actor, &job.id).await?;
    assert_eq!(job.state, "complete", "{:?}", job.error);
    let scan_seconds = scan_started.elapsed().as_secs_f64();
    let total = services
        .catalog
        .recordings(
            &actor,
            Search {
                limit: Some(1),
                ..Search::default()
            },
        )
        .await?
        .total;

    let mut provider = ProviderProcess::start(
        &variable(
            "BENCH_PROVIDER",
            "/workspace/apps/web/e2e/latency/fake-provider.mjs",
        ),
        &variable("BENCH_AUDIO", "/music/remote/remote.mp3"),
        group * 6,
    )?;
    let offered = services
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Bench provider".into(),
                base_url: provider.address.clone(),
            },
        )
        .await?;
    let installation = services
        .plugins
        .install(
            &actor,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: offered.id,
                service_revision: offered.revision,
                bearer_token: Some(provider.token.clone()),
            },
        )
        .await?;
    let mut adopted = Vec::new();
    for index in 0..group * 6 {
        let name = format!("bench-remote-{index}");
        let sides: Vec<String> = if (2..4).contains(&(index / group)) {
            vec![format!("audio/{name}/a"), format!("audio/{name}/b")]
        } else {
            vec![format!("audio/{name}/a")]
        };
        provider.sources(&name, &sides.iter().map(String::as_str).collect::<Vec<_>>())?;
        adopted.push(
            services
                .catalog
                .adopt_provider(
                    &actor,
                    &installation.id,
                    AdoptProviderRecording {
                        installation_revision: installation.revision,
                        provider_recording_id: name,
                    },
                )
                .await?
                .id,
        );
    }

    let monitor = Monitor::open()?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (player, owner) = Player::native(services.clone());
    let attached = player
        .claim(&actor, player.inspect(&actor).await?.epoch)
        .await?;
    let state = player.subscribe(&actor, attached.lease).await?;
    let measurements = Measurements::open(&out, warmup)?;
    let mut rig = Rig::new(
        player,
        actor.clone(),
        attached.lease,
        state,
        monitor,
        measurements,
    );

    for (format, offset) in [("FLAC", 0_usize), ("MP3", 1), ("M4A", 2)] {
        for n in 0..group {
            let index = ((n * 97 + 13) % format_span) * 3 + offset;
            let title = format!("Bench {format} {index:05}");
            let id = find(&services, &actor, &title).await;
            let result = rig.start_once(&id).await;
            {
                let sample = n;
                let fields = start_fields("local_start", format, 0, sample, &title, &result, "");
                rig.samples.record(n, &fields)?;
            }
            rig.settle().await?;
        }
    }

    for (block, latency) in [(0_usize, 0_u64), (1, rtt)] {
        provider.latency(latency)?;
        for n in 0..group {
            let id = adopted[block * group + n].clone();
            let before = provider.counters()?;
            let result = rig.start_once(&id).await;
            let after = provider.counters()?;
            {
                let sample = n;
                let note = format!(
                    "provider_sources={};provider_audio={}",
                    after.0 - before.0,
                    after.1 - before.1
                );
                let fields = start_fields(
                    "remote_start",
                    "direct",
                    latency,
                    sample,
                    "mp3",
                    &result,
                    &note,
                );
                rig.samples.record(n, &fields)?;
            }
            rig.settle().await?;
        }
        provider.latency(0)?;
    }

    for (variant, block, latency) in [
        ("cached_alternate", 2_usize, 0_u64),
        ("cached_alternate", 3, rtt),
        ("refreshed_alternate", 4, 0),
        ("refreshed_alternate", 5, rtt),
    ] {
        let refreshed = variant == "refreshed_alternate";
        for n in 0..group {
            let id = adopted[block * group + n].clone();
            let name = format!("bench-remote-{}", block * group + n);
            let a = format!("audio/{name}/a");
            let b = format!("audio/{name}/b");
            provider.latency(0)?;
            provider.pace(&a)?;
            if !refreshed {
                provider.pace(&b)?;
            }
            let before = provider.counters()?;
            let scenario = Recovery {
                recording: &id,
                name: &name,
                a: &a,
                b: &b,
                refreshed,
                latency,
            };
            let outcome = recovery_once(&mut rig, &mut provider, &scenario).await;
            let after = provider.counters()?;
            {
                let sample = n;
                let note = format!(
                    "provider_sources={};provider_audio={}",
                    after.0 - before.0,
                    after.1 - before.1
                );
                let head = vec![
                    "recovery".to_string(),
                    variant.to_string(),
                    latency.to_string(),
                    sample.to_string(),
                    "mp3".to_string(),
                ];
                let tail = match &outcome {
                    Ok((dropped, silence, audible)) => vec![
                        "1".to_string(),
                        milliseconds(*dropped, *audible),
                        silence
                            .map(|at| milliseconds(*dropped, at))
                            .unwrap_or_default(),
                        String::new(),
                        String::new(),
                        note,
                    ],
                    Err(error) => vec![
                        "0".to_string(),
                        "0".to_string(),
                        String::new(),
                        String::new(),
                        String::new(),
                        failure(error),
                    ],
                };
                rig.samples.record(n, &[head, tail].concat())?;
            }
            provider.drop_source(&a)?;
            provider.drop_source(&b)?;
            provider.latency(0)?;
            rig.settle().await?;
        }
    }

    rig.monitor.disarm();
    owner.close().await?;
    rig.monitor.finish()?;
    provider.finish()?;
    let (measured_failures, warmup_failures) = rig.samples.finish()?;
    std::fs::write(
        out.join("native-environment.json"),
        serde_json::to_string_pretty(&json!({
            "library_scan_seconds": scan_seconds,
            "recordings_total": total,
            "samples": samples,
            "warmup": warmup,
            "rtt_ms": rtt,
            "measured_failures": measured_failures,
            "warmup_failures": warmup_failures,
        }))?,
    )?;
    drop(directory);
    if measured_failures != 0 || warmup_failures != 0 {
        return Err("benchmark_samples_failed".into());
    }
    Ok(())
}
