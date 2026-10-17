#![cfg(unix)]

use std::{
    fs::{DirBuilder, Permissions},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
};

use hito_community::{Config, Error, Service};

#[tokio::test]
async fn a_replaceable_ancestor_is_rejected_before_mutating_private_state() {
    let fixture = tempfile::tempdir().unwrap();
    let replaceable = fixture.path().join("replaceable");
    std::fs::create_dir(&replaceable).unwrap();
    std::fs::set_permissions(&replaceable, Permissions::from_mode(0o777)).unwrap();
    let state_directory = replaceable.join("community");
    DirBuilder::new()
        .mode(0o700)
        .create(&state_directory)
        .unwrap();
    assert!(
        std::fs::read_dir(&state_directory)
            .unwrap()
            .next()
            .is_none()
    );

    let rejected = match Service::open(Config {
        state_directory: state_directory.clone(),
        admin_token: "17a0427d104d87ad8adce74f1fbbe334cb4969d697d0fe4d00bc58e15863a5ad".to_owned(),
    })
    .await
    {
        Err(Error::StateUnavailable) => true,
        Err(_) => panic!("State admission failed for an unexpected reason"),
        Ok(service) => {
            service.close().await;
            false
        }
    };

    assert!(
        rejected,
        "A nonsticky writable ancestor must prevent startup"
    );
    assert!(
        std::fs::read_dir(&state_directory)
            .unwrap()
            .next()
            .is_none(),
        "Rejected startup must preserve the empty state directory"
    );
}
