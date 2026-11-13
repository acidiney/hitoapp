use crate::{
    account::exchange,
    artist_review_fixture::origin_counts,
    credit_member_review_credentials::private_snapshot,
    credit_member_review_preview_fixture::{Context, Library, accepted, private},
    provider_fixture::ProviderServer,
};
use axum::http::{HeaderMap, StatusCode};
use futures_util::FutureExt;
use hito_core::{
    ArtistReviewContext, CreditMemberReviewContext,
    test_support::{
        artist_catalog_discovery_service as catalog, hang_guard::hang_guard, playlist_service,
    },
};
use hito_plugin_sdk::{CreditRole, ProviderRecording};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

const SOLO: &str = "Owned solo main qualification";
type Response = (StatusCode, HeaderMap, Value);

struct Checkpoint {
    state: Value,
    peer: Value,
    provider_requests: usize,
    origin: Vec<usize>,
}

struct Observed {
    text_credit: ArtistReviewContext,
    whole: ArtistReviewContext,
    member: CreditMemberReviewContext,
    fresh_whole: ArtistReviewContext,
    whole_association: Value,
    denied: Response,
    confirmed: Response,
    replayed: Response,
    rejected: Response,
    baseline: Checkpoint,
    after_denial: Checkpoint,
    after_member: Checkpoint,
    after_replay: Checkpoint,
    after_rejection: Checkpoint,
    requests_before_whole_confirmation: usize,
    unexpected: usize,
}

#[tokio::test]
async fn a_main_member_with_the_whole_artist_id_keeps_a_distinct_confirmation_and_cannot_replace_the_whole_choice()
 {
    let mut library = Library::new();
    let mut origin = None;
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(
            hang_guard(
                "owned Main target qualification setup",
                Box::pin(async {
                    let context = library.prepare().await;
                    origin = Some(ProviderServer::start().await);
                    context
                }),
            )
            .await,
        );
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(
                &library,
                context.as_ref().unwrap(),
                origin.as_ref().unwrap(),
            ),
        )
        .await
        .expect("The Main target qualification must finish within the existing workflow deadline")
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
    assert_eq!(
        found.text_credit.recording_id, found.whole.recording_id,
        "The structured provider observation must retain the plain-text recording identity"
    );
    assert_eq!(
        found.text_credit.artist_id, found.whole.artist_id,
        "Text-credit adoption must retain the existing whole-credit account artist ID"
    );
    let raw_members = found.baseline.state["metadata"]["credited_artists"]
        .as_array()
        .unwrap();
    assert_eq!(raw_members.len(), 2);
    assert!(raw_members.iter().all(|member| member["role"] == "main"));
    assert!(found.baseline.state["metadata"]["artist"]["personal"].is_null());
    assert!(matches!(found.member.role, CreditRole::Main));
    assert_eq!(found.member.credited_name, SOLO);
    assert_eq!(
        found.member.artist_id, found.whole.artist_id,
        "The tested Main member must share the exact account artist ID with the whole credit"
    );
    assert!(
        found.fresh_whole == found.whole,
        "The rejected choice must use a still-current whole-credit context"
    );
    assert_eq!(found.denied.0, StatusCode::NOT_FOUND);
    private(&found.denied.1);
    assert_eq!(found.denied.2, json!({"error": "Resource not found"}));
    assert_eq!(
        found.after_denial.state, found.baseline.state,
        "A peer must be denied before any association or private-state write"
    );
    assert_eq!(found.confirmed.0, StatusCode::OK);
    private(&found.confirmed.1);
    assert_eq!(found.replayed.0, StatusCode::OK);
    private(&found.replayed.1);
    let whole_id = found.whole_association["id"].as_str().unwrap();
    let member_id = found.confirmed.2["id"].as_str().unwrap();
    assert!(Uuid::parse_str(whole_id).is_ok());
    assert!(Uuid::parse_str(member_id).is_ok());
    assert_ne!(
        member_id, whole_id,
        "Equal artist and provider IDs must not deduplicate a Main member into a whole-credit association"
    );
    assert_eq!(
        found.whole_association["provider_artist_id"],
        catalog::FIRST_ARTIST
    );
    assert_eq!(found.whole_association["artist_id"], found.whole.artist_id);
    let mut expected_member = found.whole_association.clone();
    expected_member["id"] = json!(member_id);
    assert_eq!(found.confirmed.2, expected_member);
    assert_eq!(
        found.replayed.2, found.confirmed.2,
        "Repeating the same Main confirmation must reuse only its own association"
    );
    assert_eq!(
        found.baseline.state["choice"]["state"]["status"],
        "selected"
    );
    assert_eq!(
        found.baseline.state["choice"]["state"]["association_id"],
        whole_id
    );
    assert_eq!(
        found.baseline.provider_requests, found.requests_before_whole_confirmation,
        "Re-establishing the valid whole confirmation and choice must make no provider HTTP requests"
    );
    let before_rows = found.baseline.state["associations"].as_array().unwrap();
    let after_rows = found.after_member.state["associations"].as_array().unwrap();
    assert_eq!(after_rows.len(), before_rows.len() + 1);
    assert_eq!(
        after_rows
            .iter()
            .filter(|row| row["id"] == member_id)
            .collect::<Vec<_>>(),
        vec![&expected_member]
    );
    let retained = after_rows
        .iter()
        .filter(|row| row["id"] != member_id)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        retained, *before_rows,
        "The newly retained Main row must leave every prior association unchanged"
    );
    assert_eq!(
        found.after_member.state["changes"]["revision"]
            .as_i64()
            .unwrap(),
        found.baseline.state["changes"]["revision"]
            .as_i64()
            .unwrap()
            + 1
    );
    assert_eq!(
        found.after_member.state["changes"]["undo"],
        json!({"association_id": member_id, "linked": false})
    );
    let mut retained_state = found.after_member.state.clone();
    retained_state["associations"] = json!(retained);
    retained_state["changes"]["revision"] = found.baseline.state["changes"]["revision"].clone();
    retained_state["changes"]["undo"] = found.baseline.state["changes"]["undo"].clone();
    assert_eq!(
        retained_state, found.baseline.state,
        "Only the explicit member row and its single private association-history change may be added after the valid solo baseline"
    );
    assert_eq!(
        found.after_replay.state, found.after_member.state,
        "A repeated confirmation must preserve the complete valid member state and association revision"
    );
    assert_eq!(found.rejected.0, StatusCode::CONFLICT);
    private(&found.rejected.1);
    assert_eq!(
        found.rejected.2,
        json!({"error": "The reviewed artist association changed"})
    );
    assert_eq!(
        found.after_rejection.state, found.after_replay.state,
        "Rejecting a Main member as a whole-credit choice must preserve the complete post-confirmation state"
    );
    for checkpoint in [
        &found.after_denial,
        &found.after_member,
        &found.after_replay,
        &found.after_rejection,
    ] {
        assert_eq!(checkpoint.peer, found.baseline.peer);
        assert_eq!(
            checkpoint.provider_requests, found.baseline.provider_requests,
            "Confirmation, repeat and denied writes must make no provider HTTP requests"
        );
        assert_eq!(checkpoint.origin, found.baseline.origin);
    }
    assert_eq!(found.unexpected, 0);
}

async fn exercise(library: &Library, owned: &Context, origin: &ProviderServer) -> Observed {
    let original = owned.snapshot().await;
    let original_playlist = original["library"]["playlist"]["playlist"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    origin
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.main-target-qualification", "name": "Owned Main target metadata",
                "protocol_version": 1, "capabilities": ["playlists"],
            }),
        )
        .await;
    let installation = owned
        .account
        .connect(&owned.app, &origin.url, "Owned Main target metadata", None)
        .await;
    let (text_playlist, text_recording) =
        import_snapshot(owned, origin, &installation, false).await;
    let text_credit: ArtistReviewContext = serde_json::from_value(
        accepted(
            &owned.app,
            &owned.account,
            "POST",
            &format!("/api/v1/recordings/{text_recording}/artist-review/context"),
            json!({"provider": owned.context["provider"]}),
        )
        .await,
    )
    .unwrap();
    let (playlist, recording) = import_snapshot(owned, origin, &installation, true).await;
    assert_ne!(playlist, text_playlist);
    assert_eq!(recording, text_recording);
    let metadata = owned
        .account
        .get(
            &owned.app,
            &format!("/api/v1/recordings/{recording}/metadata"),
        )
        .await;
    assert!(metadata["artist"]["personal"].is_null());
    assert_eq!(metadata["credited_artists"].as_array().unwrap().len(), 2);
    let artist = metadata["credited_artists"][0]["artist_id"]
        .as_str()
        .unwrap();
    let whole_path = format!("/api/v1/recordings/{recording}/artist-review");
    let member_path =
        format!("/api/v1/recordings/{recording}/credit-members/{artist}/artist-review");
    let choice_path = format!("/api/v1/recordings/{recording}/artist-identity");
    let provider = owned.context["provider"].clone();
    let whole: ArtistReviewContext = serde_json::from_value(
        accepted(
            &owned.app,
            &owned.account,
            "POST",
            &format!("{whole_path}/context"),
            json!({"provider": provider}),
        )
        .await,
    )
    .unwrap();
    let requests_before_whole_confirmation = library.provider.as_ref().unwrap().requests().len();
    let whole_association = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{whole_path}/confirm"),
        json!({"context": whole, "provider_artist_id": catalog::FIRST_ARTIST}),
    )
    .await;
    let current_choice = owned.account.get(&owned.app, &choice_path).await;
    accepted(
        &owned.app,
        &owned.account,
        "PUT",
        &choice_path,
        json!({"request_id": Uuid::new_v4(), "expected_revision": current_choice["revision"],
            "association_id": whole_association["id"], "context": whole}),
    )
    .await;
    let member: CreditMemberReviewContext = serde_json::from_value(
        accepted(
            &owned.app,
            &owned.account,
            "POST",
            &format!("{member_path}/context"),
            json!({"provider": provider}),
        )
        .await,
    )
    .unwrap();
    let baseline = checkpoint(
        library,
        owned,
        origin,
        &recording,
        &playlist,
        &original_playlist,
    )
    .await;
    let body = json!({"context": member, "provider_artist_id": catalog::FIRST_ARTIST});
    let path = format!("{member_path}/confirm");
    let denied = exchange(
        &owned.app,
        "POST",
        &path,
        body.clone(),
        owned.peer.headers(),
    )
    .await;
    let after_denial = checkpoint(
        library,
        owned,
        origin,
        &recording,
        &playlist,
        &original_playlist,
    )
    .await;
    let confirmed = exchange(
        &owned.app,
        "POST",
        &path,
        body.clone(),
        owned.account.headers(),
    )
    .await;
    let after_member = checkpoint(
        library,
        owned,
        origin,
        &recording,
        &playlist,
        &original_playlist,
    )
    .await;
    let replayed = exchange(&owned.app, "POST", &path, body, owned.account.headers()).await;
    let after_replay = checkpoint(
        library,
        owned,
        origin,
        &recording,
        &playlist,
        &original_playlist,
    )
    .await;
    let fresh_whole: ArtistReviewContext = serde_json::from_value(
        accepted(
            &owned.app,
            &owned.account,
            "POST",
            &format!("{whole_path}/context"),
            json!({"provider": provider}),
        )
        .await,
    )
    .unwrap();
    let rejected = exchange(&owned.app, "PUT", &choice_path,
        json!({"request_id": Uuid::new_v4(), "expected_revision": after_replay.state["choice"]["revision"],
            "association_id": confirmed.2["id"], "context": fresh_whole}), owned.account.headers()).await;
    let after_rejection = checkpoint(
        library,
        owned,
        origin,
        &recording,
        &playlist,
        &original_playlist,
    )
    .await;
    Observed {
        text_credit,
        whole,
        member,
        fresh_whole,
        whole_association,
        denied,
        confirmed,
        replayed,
        rejected,
        baseline,
        after_denial,
        after_member,
        after_replay,
        after_rejection,
        requests_before_whole_confirmation,
        unexpected: library.provider.as_ref().unwrap().unexpected(),
    }
}

async fn checkpoint(
    library: &Library,
    owned: &Context,
    origin: &ProviderServer,
    recording: &str,
    playlist: &str,
    original_playlist: &str,
) -> Checkpoint {
    let mut state = private_snapshot(&owned.app, &owned.account, recording, playlist).await;
    state["original_library"] = private_snapshot(
        &owned.app,
        &owned.account,
        owned.context["recording_id"].as_str().unwrap(),
        original_playlist,
    )
    .await;
    state["playlists"] = owned.account.get(&owned.app, "/api/v1/playlists").await;
    state["activity"] = owned.account.get(&owned.app, "/api/v1/activity").await;
    state["plugins"] = owned.account.get(&owned.app, "/api/v1/plugins").await;
    state["changes"] = owned
        .account
        .get(
            &owned.app,
            &format!("/api/v1/recordings/{recording}/artist-association-changes"),
        )
        .await;
    let peer = json!({
        "library": owned.peer_snapshot().await,
        "groups": owned.peer.get(&owned.app, "/api/v1/artist-identity-groups").await,
        "settings": owned.peer.get(&owned.app, "/api/v1/metadata/settings").await,
        "events": owned.peer.get(&owned.app, "/api/v1/sync?cursor=0").await,
    });
    let mut origins = library.origin_counts().await;
    origins.extend(origin_counts(origin).await);
    Checkpoint {
        state,
        peer,
        provider_requests: library.provider.as_ref().unwrap().requests().len(),
        origin: origins,
    }
}

async fn import_snapshot(
    owned: &Context,
    origin: &ProviderServer,
    installation: &Value,
    structured: bool,
) -> (String, String) {
    let mut recording = json!({
        "id": "owned-main-adoption-recording", "title": "Owned Main adoption recording",
        "artist": SOLO, "duration_ms": 100_000,
    });
    if structured {
        recording["artists"] = json!([
            {"name": SOLO, "role": "main", "join": " & "},
            {"name": "Owned second Main qualification", "role": "main"},
        ]);
    }
    let typed: ProviderRecording = serde_json::from_value(recording.clone()).unwrap();
    typed.validate().unwrap();
    let snapshot = if structured {
        "owned-main-adoption:2"
    } else {
        "owned-main-adoption:1"
    };
    playlist_service::playlist(
        origin,
        "owned-main-adoption",
        snapshot,
        vec![json!({
            "occurrence_id": "owned-main-adoption-occurrence",
            "item": {"kind": "recording", "recording": recording},
        })],
    )
    .await;
    let receipt = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({"request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/owned-main-adoption"}),
    )
    .await;
    let playlist = receipt["playlist_id"].as_str().unwrap().to_owned();
    let copy = owned
        .account
        .get(&owned.app, &format!("/api/v1/playlists/{playlist}"))
        .await;
    assert_eq!(copy["entries"].as_array().unwrap().len(), 1);
    let recording = copy["entries"][0]["recording_id"]
        .as_str()
        .unwrap()
        .to_owned();
    (playlist, recording)
}
