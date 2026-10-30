use super::fixture::World;
use hito_core::DeleteAccount;
use hito_core::test_support::metadata_tag_logs::CapturedLogs;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn the_deletion_log_names_no_account_and_reports_the_outcome_with_counts() {
    let world = World::empty().await;
    let admin = world.actor(&world.admin).await;
    let logs = CapturedLogs::default();
    world
        .hito
        .identity
        .delete_account(
            &admin,
            &world.member.user.id,
            DeleteAccount {
                username: "erasable-person".into(),
            },
        )
        .with_subscriber(logs.subscriber())
        .await
        .unwrap();
    let text = logs.text();
    assert!(text.contains("Account deleted"), "{text}");
    assert!(
        text.contains("outcome") && text.contains("rows_removed"),
        "{text}"
    );
    assert!(text.contains("request_id"), "{text}");
    for secret in [
        world.member.user.id.as_str(),
        world.admin.user.id.as_str(),
        "erasable-person",
        world.admin.token.as_str(),
    ] {
        assert!(
            !text.contains(secret),
            "the log must not contain {secret}: {text}"
        );
    }
}
