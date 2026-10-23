#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "bootstrap_support/inputs.rs"]
mod inputs;
#[path = "bootstrap_support/logging.rs"]
mod logging;
#[path = "bootstrap_support/process.rs"]
mod process;

use inputs::{Fixture, PRIVATE, PROVIDER_TOKEN, Snapshot, catalog_with};
use serde_json::{Value, json};
use std::{
    ffi::{CString, OsString},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::{PermissionsExt, symlink},
        },
    },
    panic::AssertUnwindSafe,
    path::Path,
};

#[test]
fn explicit_native_arguments_are_required_before_files_or_binding() {
    let fixture = Fixture::new();
    let complete = fixture.arguments();
    let mut cases = vec![(
        "no_arguments",
        Vec::new(),
        false,
        "missing_provider_token_file",
    )];
    for (index, name, reason) in [
        (0, "missing_provider", "missing_provider_token_file"),
        (2, "missing_service", "missing_service_token_file"),
        (4, "missing_receipt", "missing_receipt_key_file"),
        (6, "missing_catalog", "missing_catalog_file"),
    ] {
        let mut arguments = complete.clone();
        drop(arguments.drain(index..index + 2));
        cases.push((name, arguments, true, reason));
    }
    let mut missing_value = complete.clone();
    missing_value.pop();
    cases.push((
        "missing_catalog_value",
        missing_value,
        true,
        "missing_catalog_file",
    ));
    let mut duplicate = complete.clone();
    duplicate.extend(complete[..2].iter().cloned());
    cases.push(("duplicate_provider", duplicate, true, "invalid_arguments"));
    let mut duplicate_listen = complete.clone();
    duplicate_listen.extend(["--listen".into(), format!("{PRIVATE}listen-value").into()]);
    cases.push((
        "duplicate_listen",
        duplicate_listen,
        true,
        "invalid_arguments",
    ));
    let mut inline = complete.clone();
    inline.extend(["--provider-token".into(), PROVIDER_TOKEN.into()]);
    cases.push((
        "unknown_inline_credential",
        inline,
        true,
        "invalid_arguments",
    ));
    let mut native_flag = complete.clone();
    native_flag.push(OsString::from_vec(
        b"--owned-private-premiumize-bootstrap-flag-\xff".to_vec(),
    ));
    cases.push((
        "native_invalid_flag",
        native_flag,
        true,
        "invalid_arguments",
    ));
    let mut relative = complete.clone();
    relative[1] = fixture
        .provider
        .file_name()
        .expect("the owned input must have a filename")
        .into();
    cases.push((
        "relative_existing_path",
        relative,
        true,
        "operator_file_path_invalid",
    ));
    let mut missing_native = complete.clone();
    missing_native[1] = fixture
        .directory
        .path()
        .join(OsString::from_vec(
            b"owned-private-premiumize-bootstrap-missing-\xff".to_vec(),
        ))
        .into_os_string();
    cases.push((
        "native_missing_path",
        missing_native,
        true,
        "operator_file_unavailable",
    ));
    cases.push((
        "invalid_listen",
        vec!["--listen".into(), format!("{PRIVATE}address").into()],
        false,
        "invalid_listen_address",
    ));
    cases.push((
        "missing_listen_value",
        vec!["--listen".into()],
        false,
        "missing_listen_address",
    ));
    assert_eq!(cases.len(), 14);
    for (name, arguments, explicit_listen, reason) in cases {
        rejected(&fixture, &arguments, explicit_listen, reason);
        println!("Premiumize bootstrap argument control: case={name}");
    }
    fifo(&fixture.provider);
    let mut unknown = complete;
    unknown.push(format!("--{PRIVATE}unknown-before-fifo").into());
    rejected(&fixture, &unknown, true, "invalid_arguments");
    println!("Premiumize bootstrap argument checkpoint: rejected_cases=15");
}

#[test]
fn unsafe_private_file_types_ownership_modes_and_lengths_fail_before_binding() {
    for (name, input, control) in [
        ("provider_symlink", Input::Provider, Control::Symlink),
        ("service_hardlink", Input::Service, Control::HardLink),
        ("receipt_directory", Input::Receipt, Control::Directory),
        ("catalog_fifo", Input::Catalog, Control::Fifo),
        ("provider_group_mode", Input::Provider, Control::Mode(0o640)),
        ("service_other_mode", Input::Service, Control::Mode(0o604)),
        ("receipt_setuid_mode", Input::Receipt, Control::Mode(0o4600)),
        (
            "catalog_foreign_owner",
            Input::Catalog,
            Control::ForeignOwner,
        ),
        ("provider_empty_file", Input::Provider, Control::Length(0)),
        (
            "provider_oversized_file",
            Input::Provider,
            Control::Length(4099),
        ),
        (
            "receipt_oversized_file",
            Input::Receipt,
            Control::Length(33),
        ),
        (
            "catalog_oversized_file",
            Input::Catalog,
            Control::Length(1_048_577),
        ),
    ] {
        let fixture = Fixture::new();
        let path = input.path(&fixture);
        let reason = match control {
            Control::Symlink => {
                let target = fixture
                    .directory
                    .path()
                    .join(format!("{PRIVATE}symlink-target"));
                std::fs::rename(path, &target)
                    .unwrap_or_else(|_| panic!("the owned symlink target must move"));
                symlink(&target, path).unwrap_or_else(|_| panic!("the owned symlink must create"));
                "operator_file_unavailable"
            }
            Control::HardLink => {
                std::fs::hard_link(
                    path,
                    fixture
                        .directory
                        .path()
                        .join(format!("{PRIVATE}hardlink-alias")),
                )
                .unwrap_or_else(|_| panic!("the owned hardlink must create"));
                "operator_file_not_private"
            }
            Control::Directory => {
                std::fs::remove_file(path).unwrap_or_else(|_| panic!("the owned file must retire"));
                std::fs::create_dir(path)
                    .unwrap_or_else(|_| panic!("the owned directory control must create"));
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                    .unwrap_or_else(|_| {
                        panic!("the owned directory control must have explicit permissions")
                    });
                "operator_file_not_private"
            }
            Control::Fifo => {
                fifo(path);
                "operator_file_not_private"
            }
            Control::Mode(mode) => {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                    .unwrap_or_else(|_| panic!("the owned mode control must apply"));
                "operator_file_not_private"
            }
            Control::ForeignOwner => {
                let file = std::fs::File::open(path)
                    .unwrap_or_else(|_| panic!("the owned owner control must open"));
                let other = if unsafe { libc::geteuid() } == 0 {
                    1
                } else {
                    0
                };
                assert_eq!(
                    unsafe { libc::fchown(file.as_raw_fd(), other, libc::gid_t::MAX) },
                    0,
                    "the owned foreign-owner fixture requires Docker chown capability"
                );
                "operator_file_not_private"
            }
            Control::Length(length) => {
                write(path, &vec![0x6b; length]);
                "operator_file_not_private"
            }
        };
        rejected(&fixture, &fixture.arguments(), true, reason);
        println!("Premiumize bootstrap private-file control: case={name}");
    }
    println!("Premiumize bootstrap private-file checkpoint: rejected_cases=12");
}

#[test]
fn normalized_credentials_and_exact_receipt_keys_fail_closed() {
    for (name, input, bytes, reason) in [
        (
            "blank_token",
            Input::Provider,
            b"\n".to_vec(),
            "credential_file_invalid",
        ),
        (
            "token_above_text_limit",
            Input::Provider,
            vec![0x6b; 4097],
            "credential_file_invalid",
        ),
        (
            "invalid_utf8_token",
            Input::Provider,
            [PROVIDER_TOKEN.as_bytes(), b"\xff"].concat(),
            "credential_file_invalid",
        ),
        (
            "internal_control_token",
            Input::Provider,
            [PROVIDER_TOKEN.as_bytes(), b"\nbody"].concat(),
            "credential_file_invalid",
        ),
        (
            "space_in_token",
            Input::Service,
            format!("{PRIVATE}space in token").into_bytes(),
            "credential_file_invalid",
        ),
        (
            "repeated_token_newline",
            Input::Service,
            format!("{PRIVATE}repeated-newline\n\n").into_bytes(),
            "credential_file_invalid",
        ),
        (
            "equal_normalized_credentials",
            Input::Service,
            format!("{PROVIDER_TOKEN}\n").into_bytes(),
            "credentials_must_differ",
        ),
        (
            "short_receipt_key",
            Input::Receipt,
            vec![0x6b; 31],
            "receipt_key_invalid",
        ),
        (
            "zero_receipt_key",
            Input::Receipt,
            vec![0; 32],
            "receipt_key_invalid",
        ),
    ] {
        let fixture = Fixture::new();
        write(input.path(&fixture), &bytes);
        rejected(&fixture, &fixture.arguments(), true, reason);
        println!("Premiumize bootstrap secret control: case={name}");
    }
    println!("Premiumize bootstrap secret checkpoint: rejected_cases=9");
}

#[test]
fn private_catalog_parse_policy_and_existing_library_bounds_fail_before_binding() {
    let mut cases = vec![(
        "malformed_catalog",
        format!("{{\"{PRIVATE}malformed\"").into_bytes(),
        "catalog_file_invalid",
    )];
    for (name, location) in [
        ("unknown_root_field", "root"),
        ("unknown_entry_field", "entry"),
        ("unknown_recording_field", "recording"),
    ] {
        let mut catalog = catalog_with(1, 0);
        let object = match location {
            "root" => &mut catalog,
            "entry" => &mut catalog["recordings"][0],
            _ => &mut catalog["recordings"][0]["recording"],
        };
        object[format!("{PRIVATE}unknown-field")] = format!("{PRIVATE}unknown-value").into();
        cases.push((name, encode(&catalog), "catalog_file_invalid"));
    }
    let mut binding = catalog_with(1, 0);
    binding["recordings"][0]["binding"]["file_bytes"] = json!(43);
    cases.push((
        "invalid_typed_binding",
        encode(&binding),
        "initialization_failed",
    ));
    let mut origin = catalog_with(1, 0);
    origin["media_origins"] = json!(["http://owned-private-premiumize-bootstrap.invalid"]);
    cases.push((
        "invalid_media_origin",
        encode(&origin),
        "media_origins_invalid",
    ));
    let mut recording = catalog_with(1, 0);
    recording["recordings"][0]["recording"]["title"] = json!("");
    cases.push((
        "invalid_sdk_recording",
        encode(&recording),
        "initialization_failed",
    ));
    cases.push((
        "empty_recordings",
        encode(&catalog_with(0, 0)),
        "initialization_failed",
    ));
    cases.push((
        "recording_count_above_library_bound",
        encode(&catalog_with(129, 0)),
        "initialization_failed",
    ));
    cases.push((
        "catalog_bytes_above_library_bound",
        encode(&catalog_with(64, 1000)),
        "initialization_failed",
    ));
    assert_eq!(cases.len(), 10);
    for (name, bytes, reason) in cases {
        assert!(
            !bytes.is_empty() && bytes.len() < 1_048_576,
            "catalog semantic controls must fit the private file bound"
        );
        let fixture = Fixture::new();
        write(&fixture.catalog, &bytes);
        rejected(&fixture, &fixture.arguments(), true, reason);
        println!("Premiumize bootstrap catalog control: case={name}");
    }
    println!("Premiumize bootstrap catalog checkpoint: rejected_cases=10");
}

fn rejected(
    fixture: &Fixture,
    arguments: &[OsString],
    explicit_listen: bool,
    reason: &'static str,
) {
    let snapshot = Snapshot::capture(fixture.directory.path());
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        process::rejected(fixture, arguments, explicit_listen)
    }));
    snapshot.assert_preserved(fixture.directory.path());
    let output = match outcome {
        Ok(output) => output,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    logging::assert_rejected(&output, fixture.directory.path(), reason);
}

fn encode(catalog: &Value) -> Vec<u8> {
    serde_json::to_vec(catalog).unwrap_or_else(|_| panic!("the owned catalog control must encode"))
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|_| panic!("the owned input control must write"));
}

fn fifo(path: &Path) {
    std::fs::remove_file(path).unwrap_or_else(|_| panic!("the owned FIFO input must retire"));
    let path = CString::new(path.as_os_str().as_bytes())
        .unwrap_or_else(|_| panic!("the owned FIFO path must encode"));
    assert_eq!(
        unsafe { libc::mkfifo(path.as_ptr(), 0o600) },
        0,
        "the owned FIFO must create"
    );
}

#[derive(Clone, Copy)]
enum Input {
    Provider,
    Service,
    Receipt,
    Catalog,
}

impl Input {
    fn path(self, fixture: &Fixture) -> &Path {
        match self {
            Self::Provider => &fixture.provider,
            Self::Service => &fixture.service,
            Self::Receipt => &fixture.receipt,
            Self::Catalog => &fixture.catalog,
        }
    }
}

enum Control {
    Symlink,
    HardLink,
    Directory,
    Fifo,
    Mode(u32),
    ForeignOwner,
    Length(usize),
}
