use hito_server::RuntimeConfig;
use std::{process::Stdio, time::Duration};
use tokio::process::Command;

const TEST: &str =
    "public_ranking_runtime_uses_only_its_trusted_origin_and_preserves_reporting_configuration";

#[tokio::test]
async fn public_ranking_runtime_uses_only_its_trusted_origin_and_preserves_reporting_configuration()
{
    if let Ok(expected) = std::env::var("HITO_RANKING_CONFIG_TEST_CHILD") {
        let config = RuntimeConfig::from_env().await.unwrap();
        assert_eq!(
            config.core.community_rankings.is_some(),
            expected.contains("rankings"),
            "Public ranking reception must depend only on its own valid trusted origin"
        );
        assert_eq!(
            config.core.community.is_some(),
            expected.contains("reports"),
            "Public ranking configuration must preserve the private reporting configuration"
        );
        hito_server::run(config, async {}).await.unwrap();
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
    let administrator_file = directory.path().join("private-ranking-admin-canary");
    for (index, (ranking_origin, reporting, expected, invalid_ranking)) in [
        (Some("http://127.0.0.1:8096/"), false, "rankings", false),
        (None, false, "disabled", false),
        (
            Some("https://rankings.invalid/"),
            true,
            "rankings,reports",
            false,
        ),
        (
            Some("https://rankings.invalid/?private-ranking-query-canary"),
            true,
            "reports",
            true,
        ),
        (Some("http://rankings.invalid/"), false, "disabled", true),
    ]
    .into_iter()
    .enumerate()
    {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TEST, "--nocapture"])
            .env_clear()
            .env("HITO_RANKING_CONFIG_TEST_CHILD", expected)
            .env(
                "HITO_DATABASE",
                directory.path().join(format!("ranking-config-{index}.db")),
            )
            .env("HITO_MUSIC_ROOT", directory.path().join("music"))
            .env("HITO_WEB_DIR", &web)
            .env("HITO_DEVICE_ID", "owned-ranking-runtime-host")
            .env("HITO_LISTEN", "127.0.0.1:0")
            .env("HITO_LOG_FORMAT", "json")
            .env(
                "HITO_COMMUNITY_ADMIN_TOKEN",
                "private-inline-ranking-token-canary",
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(origin) = ranking_origin {
            command.env("HITO_COMMUNITY_RANKINGS_ORIGIN", origin);
        }
        if reporting {
            command
                .env("HITO_COMMUNITY_ORIGIN", "http://127.0.0.1:8095/")
                .env("HITO_COMMUNITY_ADMIN_TOKEN_FILE", &administrator_file);
        }
        let output = tokio::time::timeout(Duration::from_secs(5), command.output())
            .await
            .unwrap()
            .unwrap();
        let successful = output.status.success();
        let output = String::from_utf8([output.stdout, output.stderr].concat()).unwrap();
        assert!(
            successful,
            "Owned ranking runtime case {index} must remain usable; child output:\n{output}"
        );
        for event in ["server_starting", "server_stopped"] {
            assert_eq!(output.matches(&format!("\"event\":\"{event}\"")).count(), 1);
        }
        assert_eq!(
            output
                .matches("\"event\":\"community_ranking_configuration_disabled\"")
                .count(),
            usize::from(invalid_ranking),
            "Invalid ranking configuration must produce one safe static warning"
        );
        assert!(!output.contains("\"event\":\"community_configuration_disabled\""));
        for private in [
            "private-ranking-query-canary",
            "private-ranking-admin-canary",
            "private-inline-ranking-token-canary",
            "rankings.invalid",
            "127.0.0.1:8096",
        ] {
            assert!(
                !output.contains(private),
                "Runtime diagnostics must not expose supplied private configuration"
            );
        }
    }
}
