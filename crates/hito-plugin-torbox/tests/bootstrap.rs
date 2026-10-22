#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[allow(dead_code)]
#[path = "runtime_support/http.rs"]
mod http;
#[allow(dead_code)]
#[path = "runtime_support/inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "runtime_support/logging.rs"]
mod logging;
#[path = "runtime_support/process.rs"]
mod process;
#[path = "bootstrap_support/snapshot.rs"]
mod snapshot;

use inputs::{Inputs, PROVIDER_TOKEN, SERVICE_TOKEN};
use snapshot::Snapshot;
use std::{
    collections::BTreeSet,
    ffi::{CString, OsString},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
    panic::AssertUnwindSafe,
    path::Path,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[test]
fn explicit_arguments_and_private_inputs_reject_before_state_or_binding() {
    let started = Instant::now();
    let mut owners = BTreeSet::new();
    for (name, reason) in [
        ("missing_mode", "missing_state_mode"),
        ("invalid_mode", "invalid_state_mode"),
        ("duplicate_mode", "invalid_arguments"),
        ("inline_credential", "invalid_arguments"),
        ("relative_token_path", "operator_file_path_invalid"),
        ("provider_symlink", "operator_file_unavailable"),
        ("service_hardlink", "operator_file_not_private"),
        ("provider_group_mode", "operator_file_not_private"),
        ("token_control_byte", "credential_file_invalid"),
        ("equal_normalized_credentials", "credentials_must_differ"),
        ("catalog_fifo", "operator_file_not_private"),
        ("unknown_catalog_field", "catalog_file_invalid"),
    ] {
        let directory = directory();
        let inputs = Inputs::create(directory.path());
        let mut arguments = inputs.arguments("create-new");
        match name {
            "missing_mode" => {
                drop(arguments.drain(8..10));
            }
            "invalid_mode" => arguments[9] = "owned-private-torbox-runtime-invalid-mode".into(),
            "duplicate_mode" => arguments.extend(["--state-mode".into(), "open-existing".into()]),
            "inline_credential" => {
                arguments.extend(["--provider-token".into(), PROVIDER_TOKEN.into()])
            }
            "relative_token_path" => arguments[1] = inputs.provider.file_name().unwrap().into(),
            "provider_symlink" => link(&inputs.provider, directory.path()),
            "service_hardlink" => std::fs::hard_link(
                &inputs.service,
                directory.path().join("owned-private-torbox-runtime-alias"),
            )
            .expect("the owned hard link must create"),
            "provider_group_mode" => {
                std::fs::set_permissions(&inputs.provider, std::fs::Permissions::from_mode(0o640))
                    .expect("the owned control mode must apply")
            }
            "token_control_byte" => {
                std::fs::write(&inputs.service, format!("{SERVICE_TOKEN}\nbody"))
                    .expect("the owned token control must write")
            }
            "equal_normalized_credentials" => {
                std::fs::write(&inputs.service, format!("{PROVIDER_TOKEN}\n"))
                    .expect("the owned equal token must write")
            }
            "catalog_fifo" => {
                std::fs::remove_file(&inputs.catalog).expect("the owned catalog must retire");
                let path = CString::new(inputs.catalog.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
            "unknown_catalog_field" => {
                let mut catalog: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(&inputs.catalog).expect("the owned catalog must read"),
                )
                .unwrap();
                catalog["owned-private-torbox-runtime-unknown-field"] =
                    "owned-private-torbox-runtime-unknown-value".into();
                std::fs::write(&inputs.catalog, serde_json::to_vec(&catalog).unwrap())
                    .expect("the owned catalog control must write");
            }
            _ => unreachable!("the private-input cohort must remain closed"),
        }
        assert!(owners.insert(rejected(directory.path(), &arguments, Some(reason))));
        assert!(
            !inputs.state.exists(),
            "preflight rejection must not create journal state"
        );
        eprintln!("torbox_bootstrap_rejection case={name}");
    }
    assert_eq!(owners.len(), 12);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the bootstrap cohort must remain bounded"
    );
    eprintln!("torbox_bootstrap_evidence preflight_rejections=12");
}

#[test]
fn journal_paths_and_explicit_creation_fail_without_replacing_operator_state() {
    let started = Instant::now();
    let mut owners = BTreeSet::new();
    for name in [
        "short_key",
        "key_symlink",
        "state_symlink",
        "existing_create_new",
    ] {
        let directory = directory();
        let inputs = Inputs::create(directory.path());
        let database = match name {
            "short_key" => {
                std::fs::write(&inputs.key, [0x6b; 31]).expect("the owned short key must write");
                None
            }
            "key_symlink" => {
                link(&inputs.key, directory.path());
                None
            }
            "state_symlink" => {
                let target = directory
                    .path()
                    .join("owned-private-torbox-runtime-state-target");
                std::fs::create_dir(&target).expect("the owned state target must create");
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))
                    .expect("the owned state target must be private");
                symlink(target, &inputs.state).expect("the owned state symlink must create");
                None
            }
            "existing_create_new" => {
                let mut child = process::Host::start(directory.path(), &inputs, "create-new");
                let address = child.address;
                let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| child.wait_ready()));
                let stopped = child.terminate_and_join();
                outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
                assert!(stopped.signalled && stopped.status.success() && stopped.stderr.is_empty());
                closed(address);
                inputs.assert_preserved(directory.path());
                assert!(owners.insert(idle_owner(&stopped.stdout, directory.path())));
                Some(inputs.database_identity())
            }
            _ => unreachable!("the journal-path cohort must remain closed"),
        };
        assert!(owners.insert(rejected(
            directory.path(),
            &inputs.arguments("create-new"),
            None
        )));
        if let Some(database) = database {
            assert_eq!(inputs.database_identity(), database);
        }
        eprintln!("torbox_bootstrap_rejection case={name}");
    }
    assert_eq!(owners.len(), 5);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the journal bootstrap cohort must remain bounded"
    );
    eprintln!("torbox_bootstrap_evidence journal_rejections=4 joined_create_owners=1");
}

fn rejected(directory: &Path, arguments: &[OsString], reason: Option<&'static str>) -> Uuid {
    let before = Snapshot::capture(directory);
    let reservation =
        TcpListener::bind("127.0.0.1:0").expect("the owned rejection listener must reserve");
    let address = reservation
        .local_addr()
        .expect("the owned rejection address must identify");
    let child = process::Host::start_with_arguments(directory, arguments, address);
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| child.rejected_and_join()));
    before.assert_preserved(directory);
    let stopped = outcome.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert!(stopped.status.code() == Some(1) && !stopped.signalled && stopped.stderr.is_empty());
    assert!(reservation.local_addr().ok() == Some(address));
    drop(reservation);
    closed(address);
    if let Some(reason) = reason {
        let events = logging::events(&stopped.stdout, directory);
        assert_eq!(events.len(), 1);
        let failed = &events[0];
        assert!(failed["level"] == "ERROR" && failed["target"] == "hito_plugin_torbox::runtime");
        logging::keys(
            &failed["fields"],
            &["event", "outcome", "reason", "elapsed_ms"],
        );
        assert!(
            failed["fields"]["event"] == "torbox_service_failed"
                && failed["fields"]["outcome"] == "failed"
                && failed["fields"]["reason"] == reason
        );
        let spans = failed["spans"]
            .as_array()
            .expect("bootstrap failure must own spans");
        assert_eq!(spans.len(), 1);
        logging::service_id(&spans[0])
    } else {
        logging::assert_missing_state(&stopped.stdout, directory)
    }
}

fn idle_owner(output: &str, directory: &Path) -> Uuid {
    let events = logging::events(output, directory);
    assert_eq!(events.len(), 8);
    let mut startup = Vec::new();
    let mut timeline = Vec::new();
    let mut owners = BTreeSet::new();
    for event in &events {
        let fields = &event["fields"];
        let name = fields["event"]
            .as_str()
            .expect("idle setup must have a safe event name");
        match name {
            "torbox_candidate_authority_finished" => {
                assert!(
                    event["level"] == "DEBUG" && event["target"] == "hito_debrid_torbox::workflow"
                );
                logging::keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                assert!(fields["outcome"] == "complete" && fields["count"] == 1);
                let spans = event["spans"]
                    .as_array()
                    .expect("authority setup must own spans");
                assert_eq!(spans.len(), 2);
                logging::workflow_span(&spans[0], "torbox_journal", "journal_open");
                logging::workflow_span(
                    &spans[1],
                    "torbox_candidate_persistence",
                    "candidate_authority",
                );
                startup.push(name);
            }
            "torbox_journal_open_finished" | "torbox_journal_close_finished" => {
                let opening = name == "torbox_journal_open_finished";
                logging::workflow(
                    event,
                    "hito_plugin_torbox::journal",
                    if opening {
                        "journal_open"
                    } else {
                        "journal_close"
                    },
                    if opening { "ready" } else { "joined" },
                    0,
                );
                if opening {
                    startup.push(name);
                } else {
                    timeline.push(name);
                }
            }
            "torbox_service_ready"
            | "torbox_shutdown_requested"
            | "torbox_http_connections_joined"
            | "torbox_service_state_joined"
            | "torbox_service_stopped" => {
                assert!(event["level"] == "INFO");
                assert!(
                    event["target"]
                        == if name == "torbox_http_connections_joined" {
                            "hito_plugin_torbox::runtime::http"
                        } else {
                            "hito_plugin_torbox::runtime"
                        }
                );
                let spans = event["spans"]
                    .as_array()
                    .expect("idle lifecycle must own spans");
                assert_eq!(spans.len(), 1);
                owners.insert(logging::service_id(&spans[0]));
                match name {
                    "torbox_shutdown_requested" => {
                        logging::keys(fields, &["event", "outcome", "signal", "elapsed_ms"]);
                        assert!(fields["outcome"] == "stopping" && fields["signal"] == "SIGTERM");
                    }
                    "torbox_http_connections_joined" => {
                        logging::keys(fields, &["event", "outcome", "count", "elapsed_ms"]);
                        assert!(
                            fields["outcome"] == "joined"
                                && fields["count"].as_u64().is_some_and(|n| n <= 16)
                        );
                    }
                    _ => {
                        logging::keys(fields, &["event", "outcome", "elapsed_ms"]);
                        assert!(
                            fields["outcome"]
                                == if name == "torbox_service_ready" {
                                    "ready"
                                } else if name == "torbox_service_stopped" {
                                    "stopped"
                                } else {
                                    "joined"
                                }
                        );
                    }
                }
                timeline.push(name);
            }
            _ => panic!("idle setup must reject unknown or provider/media event classes"),
        }
    }
    assert!(
        startup
            == [
                "torbox_candidate_authority_finished",
                "torbox_journal_open_finished"
            ]
    );
    assert!(
        timeline
            == [
                "torbox_service_ready",
                "torbox_shutdown_requested",
                "torbox_http_connections_joined",
                "torbox_journal_close_finished",
                "torbox_service_state_joined",
                "torbox_service_stopped"
            ]
    );
    assert_eq!(owners.len(), 1);
    *owners
        .first()
        .expect("idle setup must have one service correlation")
}

fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("owned-private-torbox-runtime-")
        .tempdir()
        .expect("the owned bootstrap directory must create")
}

fn link(path: &Path, directory: &Path) {
    let target = directory.join("owned-private-torbox-runtime-link-target");
    std::fs::rename(path, &target).expect("the owned symlink target must move");
    symlink(target, path).expect("the owned input symlink must create");
}

fn closed(address: SocketAddr) {
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
        "rejected or joined bootstrap must not retain a listener"
    );
}
