#![cfg(all(target_os = "linux", not(target_env = "uclibc")))]

#[path = "bootstrap_support/inputs.rs"]
mod inputs;
#[path = "bootstrap_support/logging.rs"]
mod logging;
#[path = "bootstrap_support/process.rs"]
mod process;

use inputs::{Fixture, Snapshot};
use std::{
    ffi::CString,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
        },
    },
    path::Path,
};

#[test]
fn unsafe_private_inputs_fail_before_binding_and_preserve_operator_files() {
    let mut cases = vec![
        (Input::Receipt, Control::Symlink),
        (Input::Receipt, Control::Short),
        (Input::Receipt, Control::TooLarge),
        (Input::Receipt, Control::Zero),
    ];
    for input in [
        Input::Provider,
        Input::Service,
        Input::Receipt,
        Input::Catalog,
    ] {
        for control in [
            Control::Symlink,
            Control::HardLink,
            Control::Permissions,
            Control::Fifo,
            Control::Directory,
            Control::TooLarge,
            Control::WrongOwner,
        ] {
            if !cases.contains(&(input, control)) {
                cases.push((input, control));
            }
        }
    }
    assert_eq!(cases.len(), 30);
    for (input, control) in cases {
        println!(
            "Owned private-input control: input={} denial={}",
            input.name(),
            control.name()
        );
        let fixture = Fixture::new();
        let path = input.path(&fixture);
        let mut paths = fixture.paths();
        let reason = match control {
            Control::Symlink => {
                let target = fixture
                    .directory
                    .path()
                    .join("owned-private-controls-link-target");
                std::fs::rename(path, &target).unwrap();
                symlink(&target, path).unwrap();
                paths.push(target);
                "operator_file_unavailable"
            }
            Control::Short => {
                assert!(input == Input::Receipt);
                std::fs::write(path, vec![0x6b; 31]).unwrap();
                "receipt_key_invalid"
            }
            Control::TooLarge => {
                std::fs::write(path, vec![0x6b; input.maximum() + 1]).unwrap();
                "operator_file_not_private"
            }
            Control::Zero => {
                assert!(input == Input::Receipt);
                std::fs::write(path, vec![0; 32]).unwrap();
                "receipt_key_invalid"
            }
            Control::HardLink => {
                let alias = fixture
                    .directory
                    .path()
                    .join("owned-private-controls-hardlink-alias");
                std::fs::hard_link(path, &alias).unwrap();
                paths.push(alias);
                "operator_file_not_private"
            }
            Control::Permissions => {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
                "operator_file_not_private"
            }
            Control::Fifo => {
                std::fs::remove_file(path).unwrap();
                let name = CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                "operator_file_not_private"
            }
            Control::Directory => {
                std::fs::remove_file(path).unwrap();
                std::fs::create_dir(path).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
                "operator_file_not_private"
            }
            Control::WrongOwner => {
                let file = std::fs::File::open(path).unwrap();
                let current = unsafe { libc::geteuid() };
                let other = if current == 0 { 1 } else { 0 };
                assert_eq!(
                    unsafe { libc::fchown(file.as_raw_fd(), other, libc::gid_t::MAX) },
                    0,
                    "the owned wrong-owner fixture requires Docker chown capability"
                );
                "operator_file_not_private"
            }
        };
        let snapshot = Snapshot::capture(&paths);
        let output = process::rejected(&fixture);
        logging::assert_rejected(&output, fixture.directory.path(), reason);
        snapshot.assert_preserved(fixture.directory.path());
    }
    println!("Owned private-input checkpoint: rejected_cases=30");
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

    fn maximum(self) -> usize {
        match self {
            Self::Provider | Self::Service => 4098,
            Self::Receipt => 32,
            Self::Catalog => 1_048_576,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Service => "service",
            Self::Receipt => "receipt",
            Self::Catalog => "catalog",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Symlink,
    Short,
    Zero,
    TooLarge,
    HardLink,
    Permissions,
    Fifo,
    Directory,
    WrongOwner,
}

impl Control {
    fn name(self) -> &'static str {
        match self {
            Self::Symlink => "symlink",
            Self::Short => "short_key",
            Self::Zero => "zero_key",
            Self::TooLarge => "oversized",
            Self::HardLink => "hardlink",
            Self::Permissions => "permissions",
            Self::Fifo => "fifo",
            Self::Directory => "directory",
            Self::WrongOwner => "wrong_owner",
        }
    }
}
