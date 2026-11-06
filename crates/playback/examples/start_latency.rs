use hito_core::{
    Actor, AdoptProviderRecording, CoreConfig, Credentials, HostDeviceId, InstallPlugin,
    OfferPluginService, Search, Services,
};
use hito_playback::{Player, PlayerAction, PlayerCommand, PlayerSnapshot};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use uuid::Uuid;

type Failure = Box<dyn std::error::Error + Send + Sync>;

fn variable(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

struct Monitor {
    child: Child,
    transitions: Arc<Mutex<Vec<(Instant, bool)>>>,
    armed: Arc<AtomicBool>,
    last_audible: Arc<Mutex<Option<Instant>>>,
}

impl Monitor {
    fn open() -> Self {
        let mut child = Command::new("parec")
            .args([
                "--device=hito-test.monitor",
                "--format=float32le",
                "--rate=48000",
                "--channels=2",
                "--latency-msec=5",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("parec must start");
        let mut stdout = child.stdout.take().unwrap();
        let transitions = Arc::new(Mutex::new(Vec::new()));
        let armed = Arc::new(AtomicBool::new(false));
        let last_audible = Arc::new(Mutex::new(None));
        let (recorded, flag, latest) = (transitions.clone(), armed.clone(), last_audible.clone());
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 1920];
            let mut state: Option<bool> = None;
            loop {
                let count = match stdout.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                let now = Instant::now();
                let audible = buffer[..count / 4 * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|bytes| f32::from_le_bytes(*bytes).abs() > 1e-4);
                if audible {
                    *latest.lock().unwrap() = Some(now);
                }
                if flag.load(Ordering::SeqCst) {
                    if state != Some(audible) {
                        recorded.lock().unwrap().push((now, audible));
                    }
                    state = Some(audible);
                } else {
                    state = None;
                }
            }
        });
        Self {
            child,
            transitions,
            armed,
            last_audible,
        }
    }

    async fn silent_for(&self, quiet: Duration) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let silent = self
                .last_audible
                .lock()
                .unwrap()
                .is_none_or(|at| at.elapsed() >= quiet);
            if silent {
                return;
            }
            assert!(Instant::now() < deadline, "the output did not fall silent");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn arm(&self) {
        self.transitions.lock().unwrap().clear();
        self.armed.store(true, Ordering::SeqCst);
    }

    fn disarm(&self) {
        self.armed.store(false, Ordering::SeqCst);
    }

    fn first_audible_after(&self, after: Instant) -> Option<Instant> {
        self.transitions
            .lock()
            .unwrap()
            .iter()
            .find(|(at, audible)| *audible && *at >= after)
            .map(|(at, _)| *at)
    }

    fn first_silence_after(&self, after: Instant) -> Option<Instant> {
        self.transitions
            .lock()
            .unwrap()
            .iter()
            .find(|(at, audible)| !*audible && *at >= after)
            .map(|(at, _)| *at)
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct ProviderProcess {
    child: Child,
    input: ChildStdin,
    events: mpsc::Receiver<Value>,
    address: String,
    token: String,
}

impl ProviderProcess {
    fn start(script: &str, audio: &str, recordings: usize) -> Self {
        let mut child = Command::new("node")
            .args([script, "--audio", audio, "--recordings"])
            .arg(recordings.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("node must start the fake provider");
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = sender.send(value);
                }
            }
        });
        let listening = events.recv_timeout(Duration::from_secs(10)).unwrap();
        Self {
            child,
            input,
            events,
            address: listening["address"].as_str().unwrap().into(),
            token: listening["token"].as_str().unwrap().into(),
        }
    }

    fn ask(&mut self, command: Value, reply: &str) -> Value {
        writeln!(self.input, "{command}").unwrap();
        self.input.flush().unwrap();
        loop {
            let value = self.events.recv_timeout(Duration::from_secs(10)).unwrap();
            if value["event"] == reply {
                return value;
            }
        }
    }

    fn latency(&mut self, milliseconds: u64) {
        self.ask(json!({"cmd": "latency", "ms": milliseconds}), "ack");
    }

    fn sources(&mut self, recording: &str, ids: &[&str]) {
        let items: Vec<Value> = ids
            .iter()
            .map(|id| json!({"id": id, "label": "Bench", "ready": true}))
            .collect();
        self.ask(
            json!({"cmd": "sources", "recording": recording, "items": items}),
            "ack",
        );
    }

    fn pace(&mut self, source: &str) {
        self.ask(json!({"cmd": "pace", "source": source}), "ack");
    }

    fn unpace(&mut self, source: &str) {
        self.ask(json!({"cmd": "unpace", "source": source}), "ack");
    }

    fn streams(&mut self, source: &str) -> u64 {
        self.ask(json!({"cmd": "streams", "source": source}), "streams")["count"]
            .as_u64()
            .unwrap()
    }

    fn drop_source(&mut self, source: &str) {
        self.ask(json!({"cmd": "drop", "source": source}), "dropped");
    }

    fn counters(&mut self) -> (u64, u64) {
        let stats = self.ask(json!({"cmd": "stats"}), "stats");
        (
            stats["counters"]["sources"].as_u64().unwrap(),
            stats["counters"]["audio"].as_u64().unwrap(),
        )
    }
}

impl Drop for ProviderProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Rig {
    player: Player,
    actor: Actor,
    lease: Uuid,
    sequence: u64,
    revision: u64,
    state: watch::Receiver<PlayerSnapshot>,
    monitor: Monitor,
    output: std::fs::File,
    warmup: usize,
}

type Started = (Instant, Option<Instant>, Instant);

impl Rig {
    async fn submit(&mut self, action: PlayerAction) -> Result<(), Failure> {
        self.sequence += 1;
        let receipt = self
            .player
            .submit(
                &self.actor,
                PlayerCommand {
                    id: Uuid::new_v4(),
                    lease: self.lease,
                    sequence: self.sequence,
                    revision: self.revision,
                    action,
                },
            )
            .await
            .map_err(|error| format!("{error:?}"))?;
        self.revision = receipt.revision;
        Ok(())
    }

    fn row(&mut self, fields: &[String]) {
        writeln!(self.output, "native,{}", fields.join(",")).unwrap();
    }

    fn sample(&self, n: usize) -> Option<usize> {
        n.checked_sub(self.warmup)
    }

    async fn settle(&mut self) {
        let _ = self.submit(PlayerAction::Pause {}).await;
        self.monitor.silent_for(Duration::from_millis(150)).await;
    }

    async fn start_once(&mut self, recording: &str) -> Result<Started, Failure> {
        self.monitor.silent_for(Duration::from_millis(150)).await;
        self.monitor.arm();
        let started = Instant::now();
        self.submit(PlayerAction::Replace {
            recordings: vec![Uuid::parse_str(recording)?],
            start: 0,
        })
        .await?;
        self.submit(PlayerAction::Resume {}).await?;
        let mut playing = None;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if playing.is_none() && self.state.borrow().playing {
                playing = Some(Instant::now());
            }
            if let Some(audible) = self.monitor.first_audible_after(started) {
                return Ok((started, playing, audible));
            }
            if Instant::now() > deadline {
                return Err("no audible output within 15 seconds".into());
            }
            tokio::time::sleep(Duration::from_micros(500)).await;
        }
    }
}

fn milliseconds(from: Instant, to: Instant) -> String {
    format!("{:.2}", to.duration_since(from).as_secs_f64() * 1000.)
}

fn failure(error: &Failure) -> String {
    error.to_string().replace([',', '\n'], ";")
}

fn start_fields(
    scenario: &str,
    variant: &str,
    rtt: u64,
    sample: usize,
    detail: &str,
    result: &Result<Started, Failure>,
    note: &str,
) -> Vec<String> {
    let head = vec![
        scenario.to_string(),
        variant.to_string(),
        rtt.to_string(),
        sample.to_string(),
        detail.to_string(),
    ];
    let tail = match result {
        Ok((started, playing, audible)) => vec![
            "1".to_string(),
            milliseconds(*started, *audible),
            String::new(),
            playing
                .map(|at| milliseconds(*started, at))
                .unwrap_or_default(),
            String::new(),
            note.to_string(),
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
    [head, tail].concat()
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

#[derive(Clone, Copy)]
struct Recovery<'a> {
    recording: &'a str,
    name: &'a str,
    a: &'a str,
    b: &'a str,
    refreshed: bool,
    latency: u64,
}

async fn recovery_once(
    rig: &mut Rig,
    provider: &mut ProviderProcess,
    scenario: &Recovery<'_>,
) -> Result<(Instant, Option<Instant>, Instant), Failure> {
    let Recovery {
        recording,
        name,
        a,
        b,
        refreshed,
        latency,
    } = *scenario;
    rig.start_once(recording).await?;
    let mut position = rig.player.subscribe_position(&rig.actor, rig.lease).await?;
    tokio::time::timeout(
        Duration::from_secs(10),
        position.wait_for(|position| position.position_ms >= 1500),
    )
    .await??;
    let mut active = a;
    if refreshed {
        provider.sources(name, &[b]);
    } else {
        if provider.streams(a) == 0 {
            active = b;
        }
        provider.unpace(if active == a { b } else { a });
    }
    provider.latency(latency);
    rig.monitor.arm();
    let dropped = Instant::now();
    provider.drop_source(active);
    let wait = Instant::now() + Duration::from_secs(15);
    let mut silence = None;
    loop {
        silence = silence.or_else(|| rig.monitor.first_silence_after(dropped));
        if let Some(gap) = silence
            && let Some(audible) = rig.monitor.first_audible_after(gap)
        {
            return Ok((dropped, silence, audible));
        }
        if Instant::now() > wait {
            return Err("no recovered audio within 15 seconds".into());
        }
        tokio::time::sleep(Duration::from_micros(500)).await;
    }
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
    );
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
        provider.sources(&name, &sides.iter().map(String::as_str).collect::<Vec<_>>());
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

    let monitor = Monitor::open();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (player, owner) = Player::native(services.clone());
    let attached = player
        .claim(&actor, player.inspect(&actor).await?.epoch)
        .await?;
    let state = player.subscribe(&actor, attached.lease).await?;
    let mut output = std::fs::File::create(out.join("native-samples.csv"))?;
    writeln!(
        output,
        "surface,scenario,variant,rtt_ms,sample,detail,ok,latency_ms,detection_ms,playing_ms,timeupdate_ms,note"
    )?;
    let mut rig = Rig {
        player,
        actor: actor.clone(),
        lease: attached.lease,
        sequence: 0,
        revision: 0,
        state,
        monitor,
        output,
        warmup,
    };

    for (format, offset) in [("FLAC", 0_usize), ("MP3", 1), ("M4A", 2)] {
        for n in 0..group {
            let index = ((n * 97 + 13) % format_span) * 3 + offset;
            let title = format!("Bench {format} {index:05}");
            let id = find(&services, &actor, &title).await;
            let result = rig.start_once(&id).await;
            if let Some(sample) = rig.sample(n) {
                let fields = start_fields("local_start", format, 0, sample, &title, &result, "");
                rig.row(&fields);
            }
            rig.settle().await;
        }
    }

    for (block, latency) in [(0_usize, 0_u64), (1, rtt)] {
        provider.latency(latency);
        for n in 0..group {
            let id = adopted[block * group + n].clone();
            let before = provider.counters();
            let result = rig.start_once(&id).await;
            let after = provider.counters();
            if let Some(sample) = rig.sample(n) {
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
                rig.row(&fields);
            }
            rig.settle().await;
        }
        provider.latency(0);
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
            provider.latency(0);
            provider.pace(&a);
            if !refreshed {
                provider.pace(&b);
            }
            let before = provider.counters();
            let scenario = Recovery {
                recording: &id,
                name: &name,
                a: &a,
                b: &b,
                refreshed,
                latency,
            };
            let outcome = recovery_once(&mut rig, &mut provider, &scenario).await;
            let after = provider.counters();
            if let Some(sample) = rig.sample(n) {
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
                rig.row(&[head, tail].concat());
            }
            provider.drop_source(&a);
            provider.drop_source(&b);
            provider.latency(0);
            rig.settle().await;
        }
    }

    rig.monitor.disarm();
    owner.close().await?;
    std::fs::write(
        out.join("native-environment.json"),
        serde_json::to_string_pretty(&json!({
            "library_scan_seconds": scan_seconds,
            "recordings_total": total,
            "samples": samples,
            "warmup": warmup,
            "rtt_ms": rtt,
        }))?,
    )?;
    drop(directory);
    Ok(())
}
