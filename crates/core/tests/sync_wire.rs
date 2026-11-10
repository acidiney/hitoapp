use hito_core::{AuthSession, Credentials, SyncBatch, SyncEvent, SyncPage, SyncReceipt, User};
use serde_json::json;

#[test]
fn shared_types_encode_client_requests_and_decode_server_replies() {
    let credentials = Credentials {
        username: "listener".into(),
        password: "owned-test-password".into(),
    };
    assert_eq!(
        serde_json::to_value(credentials).unwrap(),
        json!({"username": "listener", "password": "owned-test-password"})
    );
    let user = json!({
        "id": "remote-account",
        "username": "listener",
        "admin": false,
        "display_name": null,
        "avatar_version": null
    });
    let account: User = serde_json::from_value(user.clone()).unwrap();
    assert_eq!(account.id, "remote-account");
    let session: AuthSession = serde_json::from_value(json!({
        "token": "owned-test-session",
        "user": user
    }))
    .unwrap();
    assert_eq!(session.user.id, account.id);
    assert_eq!(session.token, "owned-test-session");

    let operation = json!({
        "id": "11111111-1111-4111-8111-111111111111",
        "action": {
            "kind": "playlist_create",
            "playlist_id": "22222222-2222-4222-8222-222222222222",
            "name": "Offline list"
        }
    });
    let request = json!({"device_id": "replica-client", "operations": [operation]});
    let batch: SyncBatch = serde_json::from_value(request.clone()).unwrap();
    assert_eq!(serde_json::to_value(batch).unwrap(), request);
    let receipt: SyncReceipt = serde_json::from_value(json!({
        "acknowledged": [operation["id"]]
    }))
    .unwrap();
    assert_eq!(receipt.acknowledged[0].to_string(), operation["id"]);
    let reply = json!({
        "operations": [{
            "sequence": 7,
            "device_id": "replica-client",
            "operation": operation
        }],
        "cursor": 7,
        "has_more": false
    });
    let page: SyncPage = serde_json::from_value(reply.clone()).unwrap();
    assert_eq!(page.operations[0].sequence(), 7);
    assert_eq!(serde_json::to_value(page).unwrap(), reply);
}

#[test]
fn incoming_sync_events_reject_ambiguous_unknown_and_incomplete_envelopes() {
    let operation = json!({
        "sequence": 7,
        "device_id": "replica-client",
        "operation": {
            "id": "11111111-1111-4111-8111-111111111111",
            "action": {
                "kind": "playlist_delete",
                "playlist_id": "22222222-2222-4222-8222-222222222222"
            }
        }
    });
    let publication = json!({
        "sequence": 11,
        "publication": {
            "kind": "playlist_imported",
            "publication_id": "33333333-3333-4333-8333-333333333333",
            "snapshot": {
                "playlist_id": "22222222-2222-4222-8222-222222222222",
                "name": "Imported list",
                "origin": {
                    "installation_id": "installation",
                    "installation_revision": 1,
                    "service_id": "service",
                    "service_revision": 1,
                    "provider": "owned-provider",
                    "provider_playlist_id": "public-list",
                    "snapshot": "revision-one",
                    "attribution": {"label": "Owned provider"},
                    "imported_at": 1
                },
                "recordings": [],
                "entries": []
            }
        }
    });
    let page = json!({
        "operations": [operation, publication],
        "cursor": 11,
        "has_more": true
    });
    let decoded: SyncPage = serde_json::from_value(page.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), page);

    let mut both = operation.clone();
    both["publication"] = publication["publication"].clone();
    let mut unknown_operation = operation.clone();
    unknown_operation["unexpected"] = json!(true);
    let mut unknown_publication = publication.clone();
    unknown_publication["unexpected"] = json!(true);
    let mut publication_with_client = publication.clone();
    publication_with_client["device_id"] = json!("replica-client");
    let mut missing_client = operation.clone();
    missing_client.as_object_mut().unwrap().remove("device_id");
    let mut missing_sequence = publication.clone();
    missing_sequence.as_object_mut().unwrap().remove("sequence");
    let mut malformed_sequence = operation.clone();
    malformed_sequence["sequence"] = json!("7");
    let mut unknown_nested = operation.clone();
    unknown_nested["operation"]["action"]["unexpected"] = json!(true);
    for (case, envelope) in [
        ("both variants", both),
        ("unknown operation field", unknown_operation),
        ("unknown publication field", unknown_publication),
        ("publication with client identity", publication_with_client),
        ("missing client identity", missing_client),
        ("missing sequence", missing_sequence),
        ("malformed sequence", malformed_sequence),
        ("unknown nested action field", unknown_nested),
        ("missing payload", json!({"sequence": 7})),
        ("null payload", json!({"sequence": 7, "publication": null})),
    ] {
        assert!(
            serde_json::from_value::<SyncEvent>(envelope).is_err(),
            "accepted {case}"
        );
    }
}

#[test]
fn client_batches_remain_strict_and_cannot_submit_server_envelopes() {
    let operation = json!({
        "id": "11111111-1111-4111-8111-111111111111",
        "action": {
            "kind": "playlist_create",
            "playlist_id": "22222222-2222-4222-8222-222222222222",
            "name": "Offline list"
        }
    });
    let valid = json!({"device_id": "replica-client", "operations": [operation]});
    assert!(serde_json::from_value::<SyncBatch>(valid.clone()).is_ok());
    for pointer in ["", "/operations/0", "/operations/0/action"] {
        let mut injected = valid.clone();
        injected
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("publication".into(), json!({"kind": "playlist_imported"}));
        assert!(serde_json::from_value::<SyncBatch>(injected).is_err());
    }
    let envelope = json!({"sequence": 7, "device_id": "replica-client", "operation": operation});
    assert!(
        serde_json::from_value::<SyncBatch>(json!({
            "device_id": "replica-client", "operations": [envelope]
        }))
        .is_err()
    );
    let renamed = json!({"client_id": "replica-client", "operations": [operation]});
    assert!(serde_json::from_value::<SyncBatch>(renamed).is_err());
}

#[tokio::test]
async fn accepted_collection_events_decode_without_changing_replay_identity() {
    let (_directory, services, actor, _library, _music) =
        hito_core::test_support::support::library("wire-test-host").await;
    let wire = json!({
        "device_id": "wire-test-client",
        "operations": [{
            "id": "11111111-1111-4111-8111-111111111111",
            "action": {
                "kind": "playlist_create",
                "playlist_id": "22222222-2222-4222-8222-222222222222",
                "name": "Owned list"
            }
        }]
    });
    let batch: SyncBatch = serde_json::from_value(wire.clone()).unwrap();
    let encoded = serde_json::to_vec(&batch).unwrap();
    let accepted = services.collection.apply(&actor, batch).await.unwrap();
    let receipt: SyncReceipt =
        serde_json::from_slice(&serde_json::to_vec(&accepted).unwrap()).unwrap();
    let page = services.collection.events(&actor, 0).await.unwrap();
    let received: SyncPage = serde_json::from_slice(&serde_json::to_vec(&page).unwrap()).unwrap();
    assert_eq!(received.operations.len(), 1);
    let SyncEvent::Operation {
        device_id,
        operation,
        ..
    } = &received.operations[0]
    else {
        panic!("the accepted operation must remain a client event");
    };
    assert_eq!(device_id, "wire-test-client");
    assert_eq!(operation.id, receipt.acknowledged[0]);
    assert_eq!(
        serde_json::to_value(operation).unwrap(),
        wire["operations"][0]
    );
    let replayed = services
        .collection
        .apply(&actor, serde_json::from_slice(&encoded).unwrap())
        .await
        .unwrap();
    assert_eq!(replayed.acknowledged, receipt.acknowledged);
    let after = services
        .collection
        .events(&actor, received.cursor)
        .await
        .unwrap();
    assert!(after.operations.is_empty());
    assert_eq!(after.cursor, received.cursor);
}
