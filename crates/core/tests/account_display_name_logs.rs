use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::{CoreConfig, Credentials, HostDeviceId, open};

const SECRET_NAME: &str = "Private Display Name 4711";

#[tokio::test(flavor = "current_thread")]
async fn changing_the_display_name_logs_the_outcome_but_never_the_name() {
    let directory = tempfile::tempdir().unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("display-name-logs").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "the first long password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let captured = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(captured.subscriber()).unwrap();
    hito.identity
        .set_display_name(&actor, Some(SECRET_NAME.into()))
        .await
        .unwrap();
    assert!(
        hito.identity
            .set_display_name(&actor, Some(format!("{SECRET_NAME}\u{202e}")))
            .await
            .is_err()
    );
    let text = captured.text();
    assert!(text.contains("Display name changed"), "{text}");
    assert!(text.contains("\"display_name_set\":true"), "{text}");
    assert!(!text.contains(SECRET_NAME), "{text}");
    assert!(!text.contains("4711"), "{text}");
}
