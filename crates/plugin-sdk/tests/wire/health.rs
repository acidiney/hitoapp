use hito_plugin_sdk::{
    FailureTracker, HEALTH_PATH, HealthReason, HealthReport, HealthState, MAX_HEALTH_BYTES,
};
use serde_json::json;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(120);

fn tracker() -> FailureTracker {
    FailureTracker::new(3, WINDOW)
}

#[test]
fn health_reports_keep_their_exact_wire_shape() {
    assert_eq!(HEALTH_PATH, "v1/health");
    assert_eq!(MAX_HEALTH_BYTES, 1024);
    let healthy: HealthReport = serde_json::from_value(json!({
        "state": "ok", "reason": null, "since": null
    }))
    .unwrap();
    healthy.validate().unwrap();
    assert_eq!(healthy, HealthReport::ok());
    assert_eq!(
        serde_json::to_value(&healthy).unwrap(),
        json!({"state": "ok", "reason": null, "since": null})
    );
    for (wire, reason) in [
        ("network", HealthReason::Network),
        ("refused", HealthReason::Refused),
    ] {
        let value = json!({"state": "degraded", "reason": wire, "since": 1_700_000_000});
        let report: HealthReport = serde_json::from_value(value.clone()).unwrap();
        report.validate().unwrap();
        assert_eq!(report.state, HealthState::Degraded);
        assert_eq!(report.reason, Some(reason));
        assert_eq!(serde_json::to_value(report).unwrap(), value);
    }
}

#[test]
fn inconsistent_or_unknown_health_reports_are_rejected() {
    for value in [
        json!({"state": "ok", "reason": "network", "since": null}),
        json!({"state": "ok", "reason": null, "since": 5}),
        json!({"state": "degraded", "reason": null, "since": 5}),
        json!({"state": "degraded", "reason": "network", "since": null}),
        json!({"state": "degraded", "reason": "network", "since": 9_007_199_254_740_992_u64}),
    ] {
        let report: HealthReport = serde_json::from_value(value.clone()).unwrap();
        assert!(report.validate().is_err(), "{value}");
    }
    for value in [
        json!({"state": "broken", "reason": null, "since": null}),
        json!({"state": "degraded", "reason": "dns", "since": 1}),
        json!({"state": "ok", "reason": null, "since": null, "detail": "private"}),
    ] {
        assert!(serde_json::from_value::<HealthReport>(value).is_err());
    }
}

#[test]
fn three_network_failures_inside_the_window_degrade_health_with_a_since_time() {
    let tracker = tracker();
    let start = Instant::now();
    tracker.record_failure_at(HealthReason::Network, start);
    tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(10));
    assert_eq!(
        tracker.report_at(start + Duration::from_secs(11), 1_000_011),
        HealthReport::ok()
    );
    tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(20));
    let report = tracker.report_at(start + Duration::from_secs(25), 1_000_025);
    assert_eq!(report.state, HealthState::Degraded);
    assert_eq!(report.reason, Some(HealthReason::Network));
    assert_eq!(report.since, Some(1_000_000));
    report.validate().unwrap();
}

#[test]
fn refused_failures_degrade_separately_and_network_wins_when_both_repeat() {
    let tracker = tracker();
    let start = Instant::now();
    for second in 0..3 {
        tracker.record_failure_at(HealthReason::Refused, start + Duration::from_secs(second));
    }
    let report = tracker.report_at(start + Duration::from_secs(4), 500);
    assert_eq!(report.reason, Some(HealthReason::Refused));
    for second in 4..7 {
        tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(second));
    }
    let report = tracker.report_at(start + Duration::from_secs(8), 500);
    assert_eq!(report.reason, Some(HealthReason::Network));
}

#[test]
fn failures_of_one_kind_do_not_add_up_to_another() {
    let tracker = tracker();
    let start = Instant::now();
    tracker.record_failure_at(HealthReason::Network, start);
    tracker.record_failure_at(HealthReason::Refused, start);
    tracker.record_failure_at(HealthReason::Network, start);
    tracker.record_failure_at(HealthReason::Refused, start);
    assert_eq!(
        tracker.report_at(start + Duration::from_secs(1), 10),
        HealthReport::ok()
    );
}

#[test]
fn one_success_recovers_health_at_once() {
    let tracker = tracker();
    let start = Instant::now();
    for second in 0..4 {
        tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(second));
    }
    assert_eq!(
        tracker.report_at(start + Duration::from_secs(5), 10).state,
        HealthState::Degraded
    );
    tracker.record_success();
    assert_eq!(
        tracker.report_at(start + Duration::from_secs(6), 10),
        HealthReport::ok()
    );
    tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(7));
    assert_eq!(
        tracker.report_at(start + Duration::from_secs(8), 10),
        HealthReport::ok()
    );
}

#[test]
fn degraded_health_expires_when_failures_leave_the_window() {
    let tracker = tracker();
    let start = Instant::now();
    for second in 0..3 {
        tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(second));
    }
    assert_eq!(
        tracker
            .report_at(start + Duration::from_secs(119), 10)
            .state,
        HealthState::Degraded
    );
    assert_eq!(
        tracker.report_at(start + Duration::from_secs(121), 10),
        HealthReport::ok()
    );
}

#[test]
fn slow_failures_spread_beyond_the_window_never_degrade() {
    let tracker = tracker();
    let start = Instant::now();
    for index in 0..50u64 {
        let now = start + Duration::from_secs(index * 70);
        tracker.record_failure_at(HealthReason::Network, now);
        assert_eq!(tracker.report_at(now, 10), HealthReport::ok(), "{index}");
    }
}

#[test]
fn memory_stays_bounded_under_a_failure_storm() {
    let tracker = tracker();
    let start = Instant::now();
    for index in 0..100_000u64 {
        tracker.record_failure_at(HealthReason::Network, start + Duration::from_millis(index));
    }
    assert!(tracker.retained() <= 6);
    let report = tracker.report_at(start + Duration::from_secs(101), 200_000);
    assert_eq!(report.reason, Some(HealthReason::Network));
    assert_eq!(report.since, Some(199_999));
}

#[test]
fn reports_contain_nothing_but_the_three_fields() {
    let tracker = tracker();
    let start = Instant::now();
    for second in 0..3 {
        tracker.record_failure_at(HealthReason::Network, start + Duration::from_secs(second));
    }
    let report = tracker.report_at(start + Duration::from_secs(3), 10);
    let encoded = serde_json::to_vec(&report).unwrap();
    assert!(encoded.len() <= MAX_HEALTH_BYTES);
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["reason", "since", "state"]);
}
