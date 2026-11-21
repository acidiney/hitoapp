use crate::{
    account::exchange,
    credit_member_review_preview_fixture::{Context, Library, accepted},
    member_search_order_library::MEMBER,
    provider_fixture::ProviderServer,
};
use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{
    CreditMemberReviewContext,
    test_support::{
        artist_catalog_discovery_service as catalog, hang_guard::hang_guard, playlist_service,
    },
};
use serde_json::{Value, json};
use std::{
    panic::{AssertUnwindSafe, resume_unwind},
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn a_member_only_artist_takes_one_identity_for_every_recording_it_is_named_in() {
    let mut library = Library::new();
    let mut context = None;
    let outcome = AssertUnwindSafe(Box::pin(async {
        context = Some(hang_guard("member credit owned setup", Box::pin(library.prepare())).await);
        tokio::time::timeout(
            Duration::from_secs(30),
            exercise(library.origin(), context.as_ref().unwrap()),
        )
        .await
        .expect("The member credit identity workflow must finish within its existing deadline")
    }))
    .catch_unwind()
    .await;
    drop(context.take());
    library.retire().await;
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

async fn second_appearance(origin: &ProviderServer, owned: &Context) -> String {
    playlist_service::playlist(
        origin,
        "owned-member-second",
        "owned-member-second:1",
        vec![json!({
            "occurrence_id": "owned-second-duet", "item": {"kind": "recording", "recording": {
                "id": "owned-member-second-duet", "title": "Owned second duet",
                "artist": "Owned second lead feat. Owned ordering guest", "duration_ms": 90_000,
                "artists": [
                    {"name": "Owned second lead", "role": "main", "join": " feat. "},
                    {"name": MEMBER, "role": "featured"}
                ]
            }}
        })],
    )
    .await;
    let plugins = owned.account.get(&owned.app, "/api/v1/plugins").await;
    let installation = plugins["installations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|installation| {
            installation
                .to_string()
                .contains("org.hito.member-order-playlist")
        })
        .unwrap()
        .clone();
    let receipt = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        json!({"request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/owned-member-second"}),
    )
    .await;
    let copy = owned
        .account
        .get(
            &owned.app,
            &format!(
                "/api/v1/playlists/{}",
                receipt["playlist_id"].as_str().unwrap()
            ),
        )
        .await;
    copy["entries"][0]["recording_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn exercise(origin: &ProviderServer, owned: &Context) {
    let context: CreditMemberReviewContext = serde_json::from_value(owned.context.clone()).unwrap();
    let association = accepted(
        &owned.app,
        &owned.account,
        "POST",
        &format!("{}/confirm", owned.member_path),
        json!({"context": context, "provider_artist_id": catalog::SECOND_ARTIST}),
    )
    .await;
    let choice_path = format!(
        "/api/v1/recordings/{}/credit-members/{}/artist-identity",
        context.recording_id, context.artist_id
    );
    let chosen = exchange(
        &owned.app,
        "PUT",
        &choice_path,
        json!({
            "request_id": Uuid::new_v4(), "expected_revision": 0,
            "association_id": association["id"], "context": context,
        }),
        owned.account.headers(),
    )
    .await;
    assert_eq!(chosen.0, StatusCode::OK);
    let group = chosen.2["selection"]["group_id"].clone();
    let second = second_appearance(origin, owned).await;
    assert_ne!(second, context.recording_id);
    let second_path = format!(
        "/api/v1/recordings/{second}/credit-members/{}/artist-identity",
        context.artist_id
    );
    let read = |path: String| async move {
        exchange(
            &owned.app,
            "GET",
            &path,
            Value::Null,
            owned.account.headers(),
        )
        .await
    };
    let before = read(second_path.clone()).await;
    assert_eq!(before.0, StatusCode::OK, "{}", before.2);
    assert_ne!(before.2["state"]["status"], "selected");

    let apply_path = format!(
        "/api/v1/artists/{}/member-artist-identity",
        context.artist_id
    );
    let identity_before = read(apply_path.clone()).await;
    assert_eq!(identity_before.0, StatusCode::OK, "{}", identity_before.2);
    assert_eq!(
        identity_before.2,
        json!({"artist_id": context.artist_id, "chosen": [{"group_id": group, "tracks": 1}], "undecided_tracks": 1})
    );
    let request_id = Uuid::new_v4();
    let request = json!({"request_id": request_id, "group_id": group});
    let applied = exchange(
        &owned.app,
        "POST",
        &apply_path,
        request.clone(),
        owned.account.headers(),
    )
    .await;
    assert_eq!(applied.0, StatusCode::OK, "{}", applied.2);
    assert_eq!(
        applied.2,
        json!({
            "request_id": request_id, "artist_id": context.artist_id, "group_id": group,
            "applied": 1, "unchanged": 1, "skipped_different": 0, "skipped_unavailable": 0,
            "next_after": null,
        })
    );
    let after = read(second_path.clone()).await.2;
    assert_eq!(after["state"]["status"], "selected");
    assert_eq!(after["state"]["group_id"], group);
    assert_eq!(
        read(choice_path.clone()).await.2["state"]["group_id"],
        group
    );

    let replay = exchange(
        &owned.app,
        "POST",
        &apply_path,
        request,
        owned.account.headers(),
    )
    .await;
    assert_eq!(replay.0, StatusCode::OK);
    assert_eq!(
        serde_json::to_vec(&replay.2).unwrap(),
        serde_json::to_vec(&applied.2).unwrap()
    );

    let again = exchange(
        &owned.app,
        "POST",
        &apply_path,
        json!({"request_id": Uuid::new_v4(), "group_id": group}),
        owned.account.headers(),
    )
    .await;
    assert_eq!(again.0, StatusCode::OK);
    assert_eq!(again.2["applied"], 0);
    assert_eq!(again.2["unchanged"], 2);

    let peer = exchange(
        &owned.app,
        "POST",
        &apply_path,
        json!({"request_id": Uuid::new_v4(), "group_id": group}),
        owned.peer.headers(),
    )
    .await;
    assert_eq!(peer.0, StatusCode::NOT_FOUND);
    let peer_read = exchange(
        &owned.app,
        "GET",
        &apply_path,
        Value::Null,
        owned.peer.headers(),
    )
    .await;
    assert_eq!(peer_read.0, StatusCode::NOT_FOUND);
    owned.assert_context_preserved().await;
}
