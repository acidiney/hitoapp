use crate::{
    account::exchange,
    credit_member_identity_reset_fixture::{Checkpoint, Reads, Response, Scenario, accepted},
    credit_member_review_preview_fixture::{Context, Library, private},
    provider_fixture::ProviderServer,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{
    ChooseCreditMemberIdentity, CreditMemberIdentityChoiceReceipt, CreditMemberReviewContext,
    ResetRecordingArtistIdentity,
    test_support::{artist_catalog_discovery_service as catalog, hang_guard::hang_guard},
};
use hito_plugin_sdk::CreditRole;
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

struct Chosen {
    context: CreditMemberReviewContext,
    association: Value,
    receipt: Value,
    path: String,
}

struct Snapshot {
    checkpoint: Checkpoint,
    main: Reads,
    featured: Reads,
}

struct Attempt {
    phase: &'static str,
    expected: StatusCode,
    response: Response,
    after: Snapshot,
}

struct Observed {
    main: Chosen,
    featured: Chosen,
    request_id: Uuid,
    before: Snapshot,
    rejected: Vec<Attempt>,
    reset: Response,
    after: Snapshot,
    recovery: Vec<Attempt>,
}

#[tokio::test]
async fn member_reset_rejections_preserve_choices_and_only_exact_requests_replay() {
    let mut library = Library::new();
    let mut context = None;
    let mut origin = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "member reset validation owned setup",
                Box::pin(async {
                    let owned = library.prepare().await;
                    origin = Some(ProviderServer::start().await);
                    owned
                }),
            )
            .await,
        );
        let scenario = hang_guard(
            "member reset validation public structured adoption",
            Box::pin(Scenario::prepare(
                context.as_ref().unwrap(),
                origin.as_ref().unwrap(),
            )),
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(
                &library,
                context.as_ref().unwrap(),
                origin.as_ref().unwrap(),
                &scenario,
            ),
        )
        .await
        .expect("The public member reset validation workflow must finish within its deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    if let Some(origin) = origin.take() {
        origin.shutdown().await;
    }
    library.retire().await;
    let found = match outcome {
        Ok(found) => found,
        Err(panic) => resume_unwind(panic),
    };
    for (reads, chosen) in [
        (&found.before.main, &found.main),
        (&found.before.featured, &found.featured),
    ] {
        assert_current(&reads.current, chosen, true, 1);
        for response in [&reads.detail, &reads.filtered] {
            assert_eq!(response.0, StatusCode::OK);
            private(&response.1);
            assert_eq!(
                response.2["group"]["id"],
                chosen.receipt["selection"]["group_id"]
            );
            assert_eq!(
                response.2["group"]["provider_artist_id"],
                chosen.association["provider_artist_id"]
            );
        }
        assert_eq!(reads.peer_detail.0, StatusCode::NOT_FOUND);
        private(&reads.peer_detail.1);
        assert_eq!(reads.peer_detail.2, json!({"error": "Resource not found"}));
    }
    assert_eq!(found.rejected.len(), 8);
    for attempt in &found.rejected {
        assert_response(attempt);
        attempt.after.assert_preserved(&found.before);
    }
    assert_eq!(found.reset.0, StatusCode::OK);
    private(&found.reset.1);
    assert_eq!(
        found.reset.2,
        json!(CreditMemberIdentityChoiceReceipt {
            request_id: found.request_id,
            recording_id: found.featured.context.recording_id.clone(),
            artist_id: found.featured.context.artist_id.clone(),
            role: CreditRole::Featured,
            revision: 2,
            selection: None,
        })
    );
    found
        .after
        .checkpoint
        .assert_private_preserved(&found.before.checkpoint);
    assert_eq!(
        found.after.checkpoint.groups,
        found.before.checkpoint.groups
    );
    assert_reads_preserved(&found.after.main, &found.before.main);
    assert_group_reads_preserved(&found.after.featured, &found.before.featured);
    assert_current(&found.after.featured.current, &found.featured, false, 2);
    assert_eq!(found.recovery.len(), 4);
    for attempt in &found.recovery {
        assert_response(attempt);
        attempt.after.assert_preserved(&found.after);
        if attempt.expected == StatusCode::OK {
            assert!(
                serde_json::to_vec(&attempt.response.2).unwrap()
                    == serde_json::to_vec(&found.reset.2).unwrap(),
                "Exact reset replay must return the original immutable receipt"
            );
        }
    }
}

async fn exercise(
    library: &Library,
    owned: &Context,
    origin: &ProviderServer,
    scenario: &Scenario,
) -> Observed {
    let main = choose(owned, &scenario.main, catalog::SECOND_ARTIST).await;
    library.provider.as_ref().unwrap().mismatched_profile();
    let featured = choose(owned, &scenario.featured, catalog::FIRST_ARTIST).await;
    assert!(matches!(main.context.role, CreditRole::Main));
    assert!(matches!(featured.context.role, CreditRole::Featured));
    assert_ne!(main.context.artist_id, featured.context.artist_id);
    assert_ne!(main.association["id"], featured.association["id"]);
    assert_ne!(
        main.receipt["selection"]["group_id"],
        featured.receipt["selection"]["group_id"]
    );
    scenario.assert_contexts_current(owned).await;
    let before = Snapshot::capture(library, owned, origin, scenario, &main, &featured).await;
    let request_id = Uuid::new_v4();
    let request = json!(ResetRecordingArtistIdentity {
        request_id,
        expected_revision: 1,
    });
    let path = format!("{}/reset", featured.path);
    let mut anonymous = owned.account.headers();
    anonymous.remove("cookie");
    let mut forged = owned.account.headers();
    forged.insert("x-hito-account", owned.peer.id.parse().unwrap());
    let mut unknown = request.clone();
    unknown["unexpected"] = json!("owned-reset-strict-decode-canary");
    let cases = [
        (
            "anonymous",
            StatusCode::UNAUTHORIZED,
            anonymous,
            request.clone(),
        ),
        (
            "forged account",
            StatusCode::FORBIDDEN,
            forged,
            request.clone(),
        ),
        (
            "private recording",
            StatusCode::NOT_FOUND,
            owned.peer.headers(),
            request.clone(),
        ),
        (
            "nil request",
            StatusCode::UNPROCESSABLE_ENTITY,
            owned.account.headers(),
            json!({"request_id": Uuid::nil(), "expected_revision": 1}),
        ),
        (
            "negative revision",
            StatusCode::UNPROCESSABLE_ENTITY,
            owned.account.headers(),
            json!({"request_id": request_id, "expected_revision": -1}),
        ),
        (
            "unknown request field",
            StatusCode::UNPROCESSABLE_ENTITY,
            owned.account.headers(),
            unknown,
        ),
        (
            "stale revision",
            StatusCode::CONFLICT,
            owned.account.headers(),
            json!({"request_id": request_id, "expected_revision": 0}),
        ),
    ];
    let mut rejected = Vec::new();
    for (phase, expected, headers, body) in cases {
        let response = exchange(&owned.app, "POST", &path, body, headers).await;
        let after = Snapshot::capture(library, owned, origin, scenario, &main, &featured).await;
        rejected.push(Attempt {
            phase,
            expected,
            response,
            after,
        });
    }
    accepted(
        &owned.app,
        &owned.account,
        "PUT",
        "/api/v1/account/pin",
        json!({"password": "owned artist review administrator password", "pin": "4321", "idle_minutes": 0}),
    )
    .await;
    let locked = accepted(
        &owned.app,
        &owned.account,
        "POST",
        "/api/v1/account/lock",
        Value::Null,
    )
    .await;
    assert_eq!(locked["locked"], true);
    let response = exchange(
        &owned.app,
        "POST",
        &path,
        request.clone(),
        owned.account.headers(),
    )
    .await;
    let unlocked = accepted(
        &owned.app,
        &owned.account,
        "POST",
        "/api/v1/account/unlock",
        json!({"pin": "4321"}),
    )
    .await;
    assert_eq!(unlocked["locked"], false);
    accepted(
        &owned.app,
        &owned.account,
        "POST",
        "/api/v1/account/pin/remove",
        json!({"password": "owned artist review administrator password"}),
    )
    .await;
    let locked_attempt = Attempt {
        phase: "locked session",
        expected: StatusCode::LOCKED,
        response,
        after: Snapshot::capture(library, owned, origin, scenario, &main, &featured).await,
    };
    let reset = exchange(
        &owned.app,
        "POST",
        &path,
        request.clone(),
        owned.account.headers(),
    )
    .await;
    let after = Snapshot::capture(library, owned, origin, scenario, &main, &featured).await;
    let mut recovery = Vec::new();
    for (phase, expected, route, body) in [
        (
            "exact replay",
            StatusCode::OK,
            path.clone(),
            request.clone(),
        ),
        (
            "changed revision with accepted UUID",
            StatusCode::CONFLICT,
            path.clone(),
            json!({"request_id": request_id, "expected_revision": 2}),
        ),
        (
            "changed member with accepted UUID",
            StatusCode::CONFLICT,
            format!("{}/reset", main.path),
            request.clone(),
        ),
        (
            "exact replay after conflicts",
            StatusCode::OK,
            path,
            request,
        ),
    ] {
        let response = exchange(&owned.app, "POST", &route, body, owned.account.headers()).await;
        let after = Snapshot::capture(library, owned, origin, scenario, &main, &featured).await;
        recovery.push(Attempt {
            phase,
            expected,
            response,
            after,
        });
    }
    rejected.push(locked_attempt);
    Observed {
        main,
        featured,
        request_id,
        before,
        rejected,
        reset,
        after,
        recovery,
    }
}

async fn choose(
    owned: &Context,
    context: &CreditMemberReviewContext,
    provider_artist_id: &str,
) -> Chosen {
    let prefix = format!(
        "/api/v1/recordings/{}/credit-members/{}",
        context.recording_id, context.artist_id
    );
    let association = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{prefix}/artist-review/confirm"),
        json!({"context": context, "provider_artist_id": provider_artist_id}),
    )
    .await;
    assert_eq!(association["artist_id"], context.artist_id);
    assert_eq!(association["provider_artist_id"], provider_artist_id);
    let path = format!("{prefix}/artist-identity");
    let request_id = Uuid::new_v4();
    let receipt = accepted(
        &owned.app,
        &owned.account,
        "PUT",
        &path,
        json!(ChooseCreditMemberIdentity {
            request_id,
            expected_revision: 0,
            association_id: Uuid::parse_str(association["id"].as_str().unwrap()).unwrap(),
            context: context.clone(),
        }),
    )
    .await;
    let group = receipt["selection"]["group_id"].as_str().unwrap();
    assert!(Uuid::parse_str(group).is_ok());
    assert_eq!(
        receipt,
        json!({
            "request_id": request_id, "recording_id": context.recording_id,
            "artist_id": context.artist_id, "role": context.role, "revision": 1,
            "selection": {"group_id": group, "association_id": association["id"]},
        })
    );
    Chosen {
        context: context.clone(),
        association,
        receipt,
        path,
    }
}

impl Snapshot {
    async fn capture(
        library: &Library,
        owned: &Context,
        origin: &ProviderServer,
        scenario: &Scenario,
        main: &Chosen,
        featured: &Chosen,
    ) -> Self {
        let main = Reads::capture(
            owned,
            scenario,
            &main.path,
            main.receipt["selection"]["group_id"].as_str().unwrap(),
        )
        .await;
        let featured = Reads::capture(
            owned,
            scenario,
            &featured.path,
            featured.receipt["selection"]["group_id"].as_str().unwrap(),
        )
        .await;
        let checkpoint = Checkpoint::capture(library, owned, origin, scenario).await;
        Self {
            checkpoint,
            main,
            featured,
        }
    }

    fn assert_preserved(&self, before: &Self) {
        self.checkpoint.assert_private_preserved(&before.checkpoint);
        assert_eq!(self.checkpoint.groups, before.checkpoint.groups);
        assert_reads_preserved(&self.main, &before.main);
        assert_reads_preserved(&self.featured, &before.featured);
    }
}

fn assert_response(attempt: &Attempt) {
    assert_eq!(attempt.response.0, attempt.expected, "{}", attempt.phase);
    private(&attempt.response.1);
    if attempt.expected != StatusCode::OK {
        assert!(attempt.response.2["error"].is_string(), "{}", attempt.phase);
        assert!(
            !attempt
                .response
                .2
                .to_string()
                .contains("owned-reset-strict-decode-canary"),
            "Rejected reset JSON must not echo unknown private fields"
        );
    }
    if attempt.expected == StatusCode::LOCKED {
        assert_eq!(attempt.response.2["code"], "locked");
    }
}

fn assert_reads_preserved(after: &Reads, before: &Reads) {
    assert_same_response(&after.current, &before.current);
    assert_group_reads_preserved(after, before);
}

fn assert_group_reads_preserved(after: &Reads, before: &Reads) {
    for (after, before) in [
        (&after.detail, &before.detail),
        (&after.filtered, &before.filtered),
        (&after.peer_detail, &before.peer_detail),
    ] {
        assert_same_response(after, before);
    }
}

fn assert_same_response(after: &Response, before: &Response) {
    assert_eq!(after.0, before.0);
    private(&after.1);
    private(&before.1);
    assert!(
        after.2 == before.2,
        "Reset validation must preserve the public member/group read"
    );
}

fn assert_current(response: &Response, chosen: &Chosen, selected: bool, revision: i64) {
    assert_eq!(response.0, StatusCode::OK);
    private(&response.1);
    let state = if selected {
        json!({"status": "selected", "group_id": chosen.receipt["selection"]["group_id"],
            "association_id": chosen.association["id"]})
    } else {
        json!({"status": "unselected"})
    };
    assert_eq!(
        response.2,
        json!({
            "recording_id": chosen.context.recording_id, "artist_id": chosen.context.artist_id,
            "role": chosen.context.role, "revision": revision, "state": state,
        })
    );
}
