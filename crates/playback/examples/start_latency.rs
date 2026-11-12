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
#[path = "start_latency/resources.rs"]
mod resources;
use capture::Monitor;
use hito_core::{
    Actor, AdoptProviderRecording, Credentials, InstallPlugin, OfferPluginService, Search, Services,
};
use hito_playback::Player;
use resources::{Metadata, OwnedRun, RunOutput, SafeCode, safe_code};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

type Failure = Box<dyn std::error::Error + Send + Sync>;

fn variable(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

async fn find(services: &Services, actor: &Actor, title: &str) -> Result<String, Failure> {
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
        .await?;
    Ok(page
        .items
        .into_iter()
        .find(|item| item.title == title)
        .ok_or(SafeCode("fixture_recording_missing"))?
        .id)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("{{\"event\":\"benchmark_panic\",\"outcome\":\"failed\"}}")
    }));
    match launch().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"event":"benchmark_run", "outcome":"failed", "code":safe_code(&error)})
            );
            std::process::ExitCode::FAILURE
        }
    }
}

async fn launch() -> Result<(), Failure> {
    let samples = variable("BENCH_SAMPLES", "100")
        .parse::<usize>()
        .map_err(|_| SafeCode("sample_count_invalid"))?;
    let warmup = variable("BENCH_WARMUP", "5")
        .parse::<usize>()
        .map_err(|_| SafeCode("warmup_count_invalid"))?;
    let rtt = variable("BENCH_RTT_MS", "40")
        .parse::<u64>()
        .map_err(|_| SafeCode("rtt_invalid"))?;
    let format_span = variable("BENCH_TRACKS", "10000")
        .parse::<usize>()
        .map_err(|_| SafeCode("track_count_invalid"))?
        / 3;
    let group = samples
        .checked_add(warmup)
        .ok_or(SafeCode("sample_count_invalid"))?;
    let remote_count = group
        .checked_mul(6)
        .ok_or(SafeCode("sample_count_invalid"))?;
    if samples == 0 || format_span == 0 {
        return Err(SafeCode("benchmark_configuration_invalid").into());
    }
    let out = PathBuf::from(variable("BENCH_OUT", "/workspace/test-results/latency"));
    let mut output = RunOutput::open(out.clone())?;
    let directory = match tempfile::tempdir_in("/dev/shm") {
        Ok(directory) => Arc::new(directory),
        Err(_) => {
            output.failed("temporary_storage_failed")?;
            return Err(SafeCode("temporary_storage_failed").into());
        }
    };
    let settings = Settings {
        samples,
        warmup,
        rtt,
        format_span,
        group,
        remote_count,
    };
    let report = OwnedRun::retain(directory)
        .execute(move |run| Box::pin(async move { pipeline(run, &out, settings).await }))
        .await;
    let successful = report.successful();
    eprintln!(
        "{}",
        serde_json::json!({"event":"benchmark_resources", "work":report.work,
        "cleanup_failures":report.cleanup_failures, "sqlite_resources_retired":report.sqlite_resources_retired,
        "sqlite_deadline_exceeded":report.sqlite_deadline_exceeded, "elapsed_ms":report.elapsed_ms})
    );
    output.finish(&report)?;
    if !successful {
        return Err(SafeCode("benchmark_run_failed").into());
    }
    Ok(())
}

struct Settings {
    samples: usize,
    warmup: usize,
    rtt: u64,
    format_span: usize,
    group: usize,
    remote_count: usize,
}

async fn pipeline(
    run: &mut OwnedRun,
    out: &std::path::Path,
    settings: Settings,
) -> Result<(), Failure> {
    let Settings {
        samples,
        warmup,
        rtt,
        format_span,
        group,
        remote_count,
    } = settings;
    let config = run.config(PathBuf::from(variable("BENCH_LIBRARY", "/music/Music")))?;
    run.services = Some(hito_core::open(config).await?);
    let services = run.services.as_ref().unwrap().clone();
    let session = services
        .identity
        .setup(Credentials {
            username: "bench-admin".into(),
            password: "latency benchmark password".into(),
        })
        .await?;
    let actor = services.identity.authenticate(&session.token).await?;
    let library = services
        .catalog
        .libraries(&actor)
        .await?
        .into_iter()
        .next()
        .ok_or(SafeCode("fixture_library_missing"))?
        .id;
    let scan_started = Instant::now();
    let job = services
        .storage
        .enqueue_scan(&actor, &library)
        .await?
        .into_iter()
        .next()
        .ok_or(SafeCode("scan_job_missing"))?;
    if !services.storage.run_next_job().await? {
        return Err(SafeCode("scan_job_not_run").into());
    }
    let job = services.storage.job(&actor, &job.id).await?;
    if job.state != "complete" {
        return Err(SafeCode("scan_job_failed").into());
    }
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

    run.metadata = Some(Metadata {
        samples,
        warmup,
        rtt_ms: rtt,
        library_scan_seconds: scan_seconds,
        recordings_total: total,
    });
    run.provider = Some(ProviderProcess::start(
        &variable(
            "BENCH_PROVIDER",
            "/workspace/apps/web/e2e/latency/fake-provider.mjs",
        ),
        &variable("BENCH_AUDIO", "/music/remote/remote.mp3"),
        remote_count,
    )?);
    let provider = run.provider.as_mut().unwrap();
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
    for index in 0..remote_count {
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

    run.monitor = Some(Monitor::open()?);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (player, owner) = Player::native(services.clone());
    run.player_owner = Some(owner);
    let attached = player
        .claim(&actor, player.inspect(&actor).await?.epoch)
        .await?;
    let state = player.subscribe(&actor, attached.lease).await?;
    let measurements = Measurements::open(out, warmup)?;
    run.rig = Some(Rig::new(
        player,
        actor.clone(),
        attached.lease,
        state,
        run.monitor.take().unwrap(),
        measurements,
    ));
    let rig = run.rig.as_mut().unwrap();

    for (format, offset) in [("FLAC", 0_usize), ("MP3", 1), ("M4A", 2)] {
        for n in 0..group {
            let index = ((n * 97 + 13) % format_span) * 3 + offset;
            let title = format!("Bench {format} {index:05}");
            let id = find(&services, &actor, &title).await?;
            let result = rig
                .start_once(&id)
                .await
                .map_err(|error| SafeCode(safe_code(&error)).into());
            {
                let sample = n;
                let fields = start_fields("local_start", format, 0, sample, format, &result, "");
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
            let result = rig
                .start_once(&id)
                .await
                .map_err(|error| SafeCode(safe_code(&error)).into());
            let observed = provider.counters();
            let after = observed.as_ref().ok().copied();
            let counters = after
                .and_then(|after| {
                    Some((
                        after.0.checked_sub(before.0)?,
                        after.1.checked_sub(before.1)?,
                    ))
                })
                .ok_or(SafeCode("provider_observation_failed"));
            let result = result.and_then(|started| {
                counters
                    .as_ref()
                    .map(|_| started)
                    .map_err(|_| SafeCode("provider_observation_failed").into())
            });
            {
                let sample = n;
                let note = counters
                    .as_ref()
                    .map(|counts| {
                        format!("provider_sources={};provider_audio={}", counts.0, counts.1)
                    })
                    .unwrap_or_default();
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
            counters?;
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
            let outcome = recovery_once(rig, provider, &scenario)
                .await
                .map_err(|error| SafeCode(safe_code(&error)).into());
            let observed = provider.counters();
            let after = observed.as_ref().ok().copied();
            let counters = after
                .and_then(|after| {
                    Some((
                        after.0.checked_sub(before.0)?,
                        after.1.checked_sub(before.1)?,
                    ))
                })
                .ok_or(SafeCode("provider_observation_failed"));
            let outcome = outcome.and_then(|started| {
                counters
                    .as_ref()
                    .map(|_| started)
                    .map_err(|_| SafeCode("provider_observation_failed").into())
            });
            {
                let sample = n;
                let note = counters
                    .as_ref()
                    .map(|counts| {
                        format!("provider_sources={};provider_audio={}", counts.0, counts.1)
                    })
                    .unwrap_or_default();
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
            counters?;
            provider.drop_source(&a)?;
            provider.drop_source(&b)?;
            provider.latency(0)?;
            rig.settle().await?;
        }
    }

    Ok(())
}
