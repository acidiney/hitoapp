use crate::{
    assert_private_logs,
    audit::Audit,
    fixture::{self, Account, Library},
    logs, provider,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use tokio::{task::JoinSet, time::timeout};
use tracing::{Subscriber, instrument::WithSubscriber};
use tracing_subscriber::prelude::*;

#[tokio::test]
#[ignore = "One controlled subscriber-lifecycle diagnostic, not a release gate"]
async fn one_scoped_subscriber_retirement_audits_the_public_held_session_workflow() {
    assert_neutral_policy();
    let provider = provider::Provider::start().await;
    let audit = Audit::default();
    let mut reads = JoinSet::new();
    let result = timeout(
        Duration::from_secs(30),
        AssertUnwindSafe(exercise(&provider, audit.clone(), &mut reads)).catch_unwind(),
    )
    .await;
    reads.shutdown().await;
    provider.finish().await;
    eprintln!("scoped_lyric_audit={}", audit.report());
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => resume_unwind(panic),
        Err(_) => panic!("The controlled scoped lyric workflow must finish within its deadline"),
    }
}

type Reply = (StatusCode, axum::http::HeaderMap, Value);

async fn exercise(provider: &provider::Provider, audit: Audit, reads: &mut JoinSet<Reply>) {
    let mut library = Library::prepare(provider).await;
    let before = library.snapshot().await;
    let capture = logs::Logs::default();
    let subscriber = tracing::Dispatch::new(capture.subscriber().with(audit.clone()));
    let old_cookie = library.account.headers()["cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    let old_token = old_cookie.strip_prefix("hito_session=").unwrap();
    let old_hash = format!("{:x}", Sha256::digest(old_token.as_bytes()));
    let gate = provider.hold_lyrics().await;
    let app = library.app.clone();
    let path = library.lookup_path();
    let body = library.lookup_body();
    let headers = library.account.headers();
    reads.spawn(
        async move { fixture::exchange(&app, "POST", &path, body, headers).await }
            .with_subscriber(subscriber.clone()),
    );
    timeout(Duration::from_secs(2), gate.entered())
        .await
        .expect("The audit must observe a real held provider body");
    provider.assert_lookups(1).await;

    let other = logs::Logs::default();
    let other_subscriber = tracing::Dispatch::new(other.subscriber().with(Audit::default()));
    tracing::dispatcher::with_default(&other_subscriber, || {});
    drop(other_subscriber);
    other.assert_empty();

    let (status, _, _) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(subscriber.clone())
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    provider.assert_lookups(1).await;
    assert_eq!(
        library
            .account
            .request(&library.app, "POST", "/api/v1/logout", Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (status, _, result) = timeout(Duration::from_secs(2), reads.join_next())
        .await
        .expect("Revocation must finish before fixture release")
        .expect("The held public request must remain owned")
        .expect("The held request must not panic");
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(result.get("document").is_none());
    timeout(Duration::from_secs(2), gate.retired())
        .await
        .expect("Revocation must retire the actual body before release");
    assert!(!gate.released());
    provider.assert_lookups(1).await;
    provider.release_lyrics().await;
    let owner = library.account.id.clone();
    library.account = Account::login(
        &library.app,
        "owner",
        "owned artist review administrator password",
    )
    .await;
    assert!(library.account.id == owner);
    let (status, _, result) = fixture::exchange(
        &library.app,
        "POST",
        &library.lookup_path(),
        library.lookup_body(),
        library.account.headers(),
    )
    .with_subscriber(subscriber)
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(result["status"] == "ready" && result["document"] == json!(provider::response(true)));
    provider.assert_lookups(2).await;
    assert!(library.snapshot().await == before);
    library.assert_original();
    assert_private_logs(
        &capture,
        &library,
        provider,
        &["conflict", "unauthorized", "ready"],
        &[&old_cookie, old_token, &old_hash],
    );
    capture.assert_core_outcomes(&["conflict", "unauthorized", "ready"]);
    audit.assert_observed();
}

fn assert_neutral_policy() {
    let plain = logs::Logs::default();
    let observed = logs::Logs::default();
    let baseline = plain.subscriber();
    let audited = observed.subscriber().with(Audit::default());
    assert_eq!(baseline.max_level_hint(), audited.max_level_hint());
    macro_rules! policy {
        ($target:expr, $level:expr, $kind:expr) => {{
            static META: tracing::Metadata<'static> = tracing::metadata! {
                name: "owned_audit_policy", target: $target, level: $level,
                fields: tracing::fieldset!(message), callsite: &SITE, kind: $kind,
            };
            static SITE: tracing::callsite::DefaultCallsite =
                tracing::callsite::DefaultCallsite::new(&META);
            let original = baseline.register_callsite(&META);
            let added = audited.register_callsite(&META);
            let allowed =
                $target.starts_with("hito_core::") || $target.starts_with("hito_server::");
            assert_eq!(
                (original.is_always(), original.is_never()),
                (allowed, !allowed)
            );
            assert_eq!(
                (original.is_always(), original.is_never()),
                (added.is_always(), added.is_never())
            );
            assert_eq!(baseline.enabled(&META), allowed);
            assert_eq!(baseline.enabled(&META), audited.enabled(&META));
        }};
    }
    macro_rules! level {
        ($target:expr, $level:expr) => {{
            policy!($target, $level, tracing::metadata::Kind::SPAN);
            policy!($target, $level, tracing::metadata::Kind::EVENT);
        }};
    }
    macro_rules! target {
        ($target:expr) => {{
            level!($target, tracing::Level::ERROR);
            level!($target, tracing::Level::WARN);
            level!($target, tracing::Level::INFO);
            level!($target, tracing::Level::DEBUG);
            level!($target, tracing::Level::TRACE);
        }};
    }
    target!("hito_core::catalog::timed_content");
    target!("hito_server::api::lyrics");
    target!("hito_server::telemetry");
    target!("reqwest");
    target!("unrelated_dependency");
    let baseline = tracing::Dispatch::new(baseline);
    let audited = tracing::Dispatch::new(audited);
    tracing::dispatcher::with_default(&baseline, probes);
    tracing::dispatcher::with_default(&audited, probes);
    plain.assert_same_capture(&observed, 15);
    eprintln!(
        "scoped_lyric_audit_policy metadata_cases=50 exact_interest=true exact_enabled=true exact_max_level=true exact_json=true"
    );
}

fn probes() {
    macro_rules! probe {
        ($target:expr, $level:expr) => {
            tracing::span!(target: $target, $level, "owned_audit_probe").in_scope(|| {
                tracing::event!(target: $target, $level, "owned audit policy probe");
            });
        };
    }
    macro_rules! target {
        ($target:expr) => {{
            probe!($target, tracing::Level::ERROR);
            probe!($target, tracing::Level::WARN);
            probe!($target, tracing::Level::INFO);
            probe!($target, tracing::Level::DEBUG);
            probe!($target, tracing::Level::TRACE);
        }};
    }
    target!("hito_core::catalog::timed_content");
    target!("hito_server::api::lyrics");
    target!("hito_server::telemetry");
    target!("reqwest");
    target!("unrelated_dependency");
}
