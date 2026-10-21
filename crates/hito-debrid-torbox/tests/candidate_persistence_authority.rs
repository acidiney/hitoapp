#![cfg(feature = "test-support")]

#[path = "audio/fixtures.rs"]
pub mod fixtures;
#[path = "audio/owned_tls.rs"]
pub mod owned_tls;
#[path = "support/mod.rs"]
pub mod provider;
#[path = "audio/scripted_media.rs"]
pub mod scripted_media;
#[path = "support/persistence_workflow.rs"]
pub mod workflow;

use hito_debrid_torbox::{CandidateContext, CandidateKey, PersistenceError};
use std::time::Duration;
use uuid::Uuid;
use workflow::Fixture;

struct Outcomes {
    foreign_same_credential_seal: Option<PersistenceError>,
    fresh_same_credential_restore_exact: bool,
    wrong_key_restore: Option<PersistenceError>,
    changed_credential_restore: Option<PersistenceError>,
    tampered_restore: Option<PersistenceError>,
}

#[tokio::test]
async fn candidate_authority_rejects_foreign_sealing_and_unauthenticated_restoration_without_io() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let fixture = Fixture::start().await;
        let outcomes = exercise(&fixture).await;
        let (requests, media_requests) = fixture.close().await;
        let mint_requests = requests
            .iter()
            .filter(|request| request.target.starts_with("/v1/api/torrents/requestdl?"))
            .count();
        println!(
            "Owned candidate authority: provider_requests={} mint_requests={mint_requests} media_requests={media_requests}",
            requests.len()
        );

        assert_preparation_only(&requests);
        assert_eq!(mint_requests, 1);
        assert_eq!(media_requests, 0);
        let outcomes = outcomes.expect("the original client clone must seal one owned candidate");
        assert_eq!(
            outcomes.foreign_same_credential_seal,
            Some(PersistenceError::Rejected)
        );
        assert!(outcomes.fresh_same_credential_restore_exact);
        assert_eq!(outcomes.wrong_key_restore, Some(PersistenceError::Rejected));
        assert_eq!(
            outcomes.changed_credential_restore,
            Some(PersistenceError::Rejected)
        );
        assert_eq!(outcomes.tampered_restore, Some(PersistenceError::Rejected));
    })
    .await
    .expect("the owned TorBox candidate-authority workflow exceeded its deadline");
}

async fn exercise(fixture: &Fixture) -> Result<Outcomes, &'static str> {
    let binding = fixtures::binding();
    let state_id = Uuid::new_v4();
    let operation_id = Uuid::new_v4();
    let handle = Uuid::new_v4();
    let context = || CandidateContext {
        state_id,
        operation_id,
        handle,
        recording_id: "owned-authority-recording",
        source_id: "owned-authority-source",
        binding: &binding,
    };
    let key = CandidateKey::new([0x53; 32]).map_err(|_| "the owned key must configure")?;
    let first = fixture.client();
    let sibling = first.clone();
    let candidate = first
        .prepare_bound_file(&binding)
        .await
        .map_err(|_| "the completed owned file must mint one candidate")?;
    let foreign = fixture.client();
    let foreign_same_credential_seal = foreign.seal_candidate(&key, context(), &candidate).err();
    let sealed = sibling
        .seal_candidate(&key, context(), &candidate)
        .map_err(|_| "a clone of the minting client must seal its candidate")?;
    drop(candidate);
    drop(first);
    drop(sibling);
    drop(foreign);

    let fresh = fixture.client();
    let restored = fresh.restore_candidate(&key, context(), sealed.as_bytes());
    let fresh_same_credential_restore_exact = restored
        .as_ref()
        .is_ok_and(|candidate| candidate.binding() == &binding);
    drop(restored);
    let wrong_key =
        CandidateKey::new([0xa7; 32]).map_err(|_| "the alternate owned key must configure")?;
    let wrong_key_restore = fresh
        .restore_candidate(&wrong_key, context(), sealed.as_bytes())
        .err();
    let changed = fixture.client_with_credential("owned-private-changed-torbox-credential");
    let changed_credential_restore = changed
        .restore_candidate(&key, context(), sealed.as_bytes())
        .err();
    let mut tampered = sealed.as_bytes().to_vec();
    let tag = tampered
        .last_mut()
        .ok_or("a sealed candidate must contain an authentication tag")?;
    *tag ^= 1;
    let tampered_restore = fresh.restore_candidate(&key, context(), &tampered).err();
    Ok(Outcomes {
        foreign_same_credential_seal,
        fresh_same_credential_restore_exact,
        wrong_key_restore,
        changed_credential_restore,
        tampered_restore,
    })
}

fn assert_preparation_only(requests: &[provider::Request]) {
    assert_eq!(requests.len(), 2);
    for (index, request) in requests.iter().enumerate() {
        assert!(request.method == "GET" && request.server_name == "api.torbox.app");
        assert!(request.body.is_empty());
        assert!(request.header("host") == Some("api.torbox.app"));
        assert!(request.header("accept") == Some("application/json"));
        assert!(request.header("accept-encoding") == Some("identity"));
        for name in [
            "cookie",
            "referer",
            "proxy-authorization",
            "x-hito-plugin-protocol",
        ] {
            assert!(request.header(name).is_none());
        }
        if index == 0 {
            assert!(request.target == "/v1/api/torrents/mylist?id=7123&bypass_cache=true");
            assert!(
                request.header("authorization")
                    == Some(format!("Bearer {}", fixtures::TOKEN).as_str())
            );
        } else {
            assert!(
                request.target
                    == "/v1/api/torrents/requestdl?token=owned-torbox-key%2F%2B%3F%3D%26-kept-private&torrent_id=7123&file_id=7&zip_link=false&redirect=false&append_name=false"
            );
            assert!(request.header("authorization").is_none());
        }
    }
}
