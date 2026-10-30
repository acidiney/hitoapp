use hito_plugin_sdk::server::init_logging;
use std::process::Command;

const CHILD_ENV: &str = "HITO_SERVER_LOGGING_CHILD";

#[test]
fn only_listed_targets_are_logged() {
    if std::env::var_os(CHILD_ENV).is_some() {
        init_logging(&["allowed_target"]).expect("the first initialisation succeeds");
        assert!(init_logging(&["allowed_target"]).is_err());
        tracing::info!(target: "allowed_target", "marker-exact");
        tracing::info!(target: "allowed_target::inner", "marker-nested");
        tracing::info!(target: "allowed_targetx", "marker-prefix-only");
        tracing::info!(target: "other_target", "marker-other");
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["only_listed_targets_are_logged", "--exact", "--nocapture"])
        .env(CHILD_ENV, "1")
        .env_remove("RUST_LOG")
        .output()
        .expect("the child test process runs");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("marker-exact"));
    assert!(stdout.contains("marker-nested"));
    assert!(!stdout.contains("marker-prefix-only"));
    assert!(!stdout.contains("marker-other"));
}
