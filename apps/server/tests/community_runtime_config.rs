use hito_server::RuntimeConfig;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

const TEST: &str = "trusted_community_configuration_is_optional_private_and_fails_closed";

#[tokio::test]
async fn trusted_community_configuration_is_optional_private_and_fails_closed() {
    if let Some(expected) = std::env::var_os("HITO_COMMUNITY_CONFIG_TEST_CHILD") {
        let config = RuntimeConfig::from_env().await.unwrap();
        assert_eq!(config.core.community.is_some(), expected == "configured");
        let services = hito_core::open(config.core).await.unwrap();
        assert!(services.healthy().await);
        assert!(
            !services
                .community
                .run_next_report_until(std::future::pending())
                .await
                .unwrap()
        );
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let token = directory.path().join("private-admin-token");
    for (index, (origin, file, configured)) in [
        (None, None, false),
        (Some("https://community.invalid/"), None, false),
        (None, Some(token.as_path()), false),
        (Some("http://127.0.0.1:8095/"), Some(token.as_path()), true),
        (
            Some("https://community.invalid/"),
            Some(token.as_path()),
            true,
        ),
        (
            Some("http://community.invalid/"),
            Some(token.as_path()),
            false,
        ),
        (
            Some("https://community.invalid/?private-query-canary"),
            Some(token.as_path()),
            false,
        ),
        (
            Some("https://private-user-canary@community.invalid/"),
            Some(token.as_path()),
            false,
        ),
        (
            Some("https://community.invalid/"),
            Some(Path::new("relative-private-token")),
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST, "--nocapture"])
            .env_clear()
            .env(
                "HITO_COMMUNITY_CONFIG_TEST_CHILD",
                if configured { "configured" } else { "disabled" },
            )
            .env(
                "HITO_DATABASE",
                directory.path().join(format!("owned-config-{index}.db")),
            )
            .env("HITO_MUSIC_ROOT", directory.path().join("music"))
            .env("HITO_WEB_DIR", &web)
            .env("HITO_DEVICE_ID", "owned-community-config-host")
            .env(
                "HITO_COMMUNITY_ADMIN_TOKEN",
                "inline-private-token-must-never-be-used",
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(origin) = origin {
            command.env("HITO_COMMUNITY_ORIGIN", origin);
        }
        if let Some(file) = file {
            command.env("HITO_COMMUNITY_ADMIN_TOKEN_FILE", file);
        }
        let output = tokio::time::timeout(Duration::from_secs(5), command.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "Owned configuration case {index} must keep the application usable"
        );
        let output = [output.stdout, output.stderr].concat();
        for private in [
            "private-query-canary",
            "private-user-canary",
            "private-admin-token",
            "relative-private-token",
            "inline-private-token-must-never-be-used",
        ] {
            assert!(
                !output
                    .windows(private.len())
                    .any(|value| value == private.as_bytes()),
                "Configuration diagnostics must not expose submitted private values"
            );
        }
    }
}
