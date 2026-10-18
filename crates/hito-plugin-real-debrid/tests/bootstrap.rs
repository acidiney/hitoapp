#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "support/runtime_bootstrap_inputs.rs"]
mod inputs;
#[path = "support/runtime_bootstrap.rs"]
mod runtime;

use inputs::{Fixture, PROVIDER_TOKEN, Snapshot, valid_catalog};
use std::{
    ffi::CString,
    net::TcpListener,
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
};

#[test]
fn unsafe_provider_and_service_files_fail_before_binding_and_preserve_operator_inputs() {
    for provider in [true, false] {
        for denial in [
            Denial::Symlink,
            Denial::HardLink,
            Denial::Permissions,
            Denial::Oversized,
            Denial::Fifo,
        ] {
            let fixture = Fixture::new();
            let credential = if provider {
                &fixture.provider
            } else {
                &fixture.service
            };
            let mut paths = fixture.paths();
            match denial {
                Denial::Symlink => {
                    let target = fixture
                        .directory
                        .path()
                        .join("private-bootstrap-link-target");
                    std::fs::rename(credential, &target).unwrap();
                    symlink(&target, credential).unwrap();
                    paths.push(target);
                }
                Denial::HardLink => {
                    let alias = fixture
                        .directory
                        .path()
                        .join("private-bootstrap-hardlink-alias");
                    std::fs::hard_link(credential, &alias).unwrap();
                    paths.push(alias);
                }
                Denial::Permissions => {
                    std::fs::set_permissions(credential, std::fs::Permissions::from_mode(0o644))
                        .unwrap();
                }
                Denial::Oversized => {
                    std::fs::write(credential, b"private-bootstrap-oversized-token".repeat(256))
                        .unwrap();
                }
                Denial::Fifo => {
                    std::fs::remove_file(credential).unwrap();
                    let path = CString::new(credential.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                }
            }
            let reason = match denial {
                Denial::Symlink => "operator_file_unavailable",
                _ => "operator_file_not_private",
            };
            let snapshot = Snapshot::capture(&paths);
            assert_rejected(&fixture, &snapshot, reason);
        }
    }
}

#[derive(Clone, Copy)]
enum Denial {
    Symlink,
    HardLink,
    Permissions,
    Oversized,
    Fifo,
}

#[test]
fn strict_private_catalog_and_separate_credentials_are_required_before_binding() {
    for field in ["root", "entry", "recording", "binding"] {
        let fixture = Fixture::new();
        let mut catalog = valid_catalog();
        let object = match field {
            "root" => &mut catalog,
            "entry" => &mut catalog["recordings"][0],
            "recording" => &mut catalog["recordings"][0]["recording"],
            "binding" => &mut catalog["recordings"][0]["binding"],
            _ => unreachable!(),
        };
        object["private-bootstrap-unknown-field"] = "private-bootstrap-unknown-value".into();
        std::fs::write(&fixture.catalog, serde_json::to_vec(&catalog).unwrap()).unwrap();
        let snapshot = Snapshot::capture(&fixture.paths());
        assert_rejected(&fixture, &snapshot, "catalog_file_invalid");
    }
    for (bytes, reason) in [
        (
            br#"{"private-bootstrap-malformed-catalog""#.to_vec(),
            "catalog_file_invalid",
        ),
        (
            b"private-bootstrap-oversized-catalog"
                .iter()
                .copied()
                .cycle()
                .take(1_048_577)
                .collect(),
            "operator_file_not_private",
        ),
    ] {
        let fixture = Fixture::new();
        std::fs::write(&fixture.catalog, bytes).unwrap();
        let snapshot = Snapshot::capture(&fixture.paths());
        assert_rejected(&fixture, &snapshot, reason);
    }
    let fixture = Fixture::new();
    std::fs::write(&fixture.service, format!("{PROVIDER_TOKEN}\n")).unwrap();
    let snapshot = Snapshot::capture(&fixture.paths());
    assert_rejected(&fixture, &snapshot, "credentials_must_differ");
}

fn assert_rejected(fixture: &Fixture, snapshot: &Snapshot, reason: &str) {
    let occupied = TcpListener::bind("127.0.0.1:0").expect("bootstrap listener setup failed");
    runtime::rejected(
        fixture.directory.path(),
        &fixture.provider,
        &fixture.service,
        &fixture.catalog,
        occupied.local_addr().unwrap(),
        reason,
    );
    snapshot.assert_preserved(fixture.directory.path());
}
