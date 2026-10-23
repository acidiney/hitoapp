use hito_core::test_support::content_service as fixture;
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, AttachReviewedProviderSource, Credentials, Error,
    InstallPlugin, OfferPluginService, PluginInstallation, ProviderCandidate, Services,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn review_does_not_request_sources_after_logout_during_details() {
    revoke_during_details(Workflow::Review, Revocation::Logout).await;
}

#[tokio::test]
async fn review_does_not_request_sources_after_connection_removal_during_details() {
    revoke_during_details(Workflow::Review, Revocation::RemoveConnection).await;
}

#[tokio::test]
async fn attachment_does_not_request_sources_after_logout_during_details() {
    revoke_during_details(Workflow::Attach, Revocation::Logout).await;
}

#[tokio::test]
async fn attachment_does_not_request_sources_after_connection_removal_during_details() {
    revoke_during_details(Workflow::Attach, Revocation::RemoveConnection).await;
}

#[tokio::test]
async fn review_does_not_request_sources_after_connection_revision_changes_during_details() {
    revoke_during_details(Workflow::Review, Revocation::ChangeConnectionRevision).await;
}

#[tokio::test]
async fn attachment_does_not_request_sources_after_connection_revision_changes_during_details() {
    revoke_during_details(Workflow::Attach, Revocation::ChangeConnectionRevision).await;
}

#[derive(Clone, Copy)]
enum Workflow {
    Review,
    Attach,
}

#[derive(Clone, Copy)]
enum Revocation {
    Logout,
    RemoveConnection,
    ChangeConnectionRevision,
}

async fn revoke_during_details(workflow: Workflow, revocation: Revocation) {
    let primary = fixture::ProviderServer::start().await;
    let alternate = fixture::ProviderServer::start().await;
    let (_directory, hito, owner, _library, _music) =
        support::library("attachment-revocation").await;
    for (server, identity) in [(&primary, "primary"), (&alternate, "alternate")] {
        server.reply("v1/recordings/detail", json!({
            "id": identity, "title": format!("Owned {identity} title"), "artist": "Owned artist"
        })).await;
        server
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": format!("{identity}-audio"), "label": "Owned source", "ready": true
                }]}),
            )
            .await;
    }
    let primary_installation = connect(&hito, &owner, &primary).await;
    let alternate_installation = connect(&hito, &owner, &alternate).await;
    let recording = hito
        .catalog
        .adopt_provider(
            &owner,
            &primary_installation.id,
            AdoptProviderRecording {
                installation_revision: primary_installation.revision,
                provider_recording_id: "primary".into(),
            },
        )
        .await
        .unwrap();
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    let playlist = Uuid::new_v4();
    support::synchronize(
        &hito,
        &owner,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "Personal title".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 4_200,
                speed: 1.25,
            },
            Action::PlaylistCreate {
                playlist_id: playlist,
                name: "Owned playlist".into(),
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 0,
            },
            Action::EntryInsert {
                playlist_id: playlist,
                entry_id: Uuid::new_v4(),
                recording_id,
                position: 1,
            },
        ],
    )
    .await;
    let session = hito
        .identity
        .login(Credentials {
            username: "admin".into(),
            password: "indexing test password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let candidate = ProviderCandidate {
        installation_id: alternate_installation.id.clone(),
        installation_revision: alternate_installation.revision,
        provider_recording_id: "alternate".into(),
    };
    let review_stamp = match workflow {
        Workflow::Review => None,
        Workflow::Attach => Some(
            hito.catalog
                .review_provider_source(&actor, &recording.id, candidate.clone())
                .await
                .unwrap()
                .review_stamp,
        ),
    };
    let playlist = playlist.to_string();
    let before = snapshot(&hito, &owner, &recording.id, &playlist).await;
    let source_requests = [
        primary.requests("v1/sources").await.len(),
        alternate.requests("v1/sources").await.len(),
    ];
    let detail_requests = alternate.requests("v1/recordings/detail").await.len();
    let gate = alternate.hold("v1/recordings/detail").await;
    let pending = {
        let catalog = hito.catalog.clone();
        let recording = recording.id.clone();
        tokio::spawn(async move {
            match workflow {
                Workflow::Review => catalog
                    .review_provider_source(&actor, &recording, candidate)
                    .await
                    .map(|_| ()),
                Workflow::Attach => catalog
                    .attach_provider_source(
                        &actor,
                        &recording,
                        AttachReviewedProviderSource {
                            candidate,
                            review_stamp: review_stamp.unwrap(),
                        },
                    )
                    .await
                    .map(|_| ()),
            }
        })
    };
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("the owned detail request must begin before revocation");
    match revocation {
        Revocation::Logout => hito.identity.logout(&session.token).await.unwrap(),
        Revocation::RemoveConnection => hito
            .plugins
            .remove(
                &owner,
                &alternate_installation.id,
                alternate_installation.revision,
            )
            .await
            .unwrap(),
        Revocation::ChangeConnectionRevision => {
            let changed = hito
                .plugins
                .clear_credential(
                    &owner,
                    &alternate_installation.id,
                    alternate_installation.revision,
                )
                .await
                .unwrap();
            assert_eq!(changed.revision, alternate_installation.revision + 1);
        }
    }
    gate.open();
    let result = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .expect("revocation must finish the workflow promptly")
        .unwrap();
    match revocation {
        Revocation::Logout => assert!(matches!(result, Err(Error::Unauthorized)), "{result:?}"),
        Revocation::RemoveConnection => {
            assert!(matches!(result, Err(Error::NotFound)), "{result:?}")
        }
        Revocation::ChangeConnectionRevision => {
            assert!(matches!(result, Err(Error::Conflict(_))), "{result:?}")
        }
    }
    assert_eq!(
        alternate.requests("v1/recordings/detail").await.len(),
        detail_requests + 1
    );
    assert_eq!(
        [
            primary.requests("v1/sources").await.len(),
            alternate.requests("v1/sources").await.len(),
        ],
        source_requests,
        "revocation during details must prevent the next outbound source request"
    );
    assert_eq!(
        snapshot(&hito, &owner, &recording.id, &playlist).await,
        before
    );
}

async fn snapshot(hito: &Services, owner: &Actor, recording: &str, playlist: &str) -> Value {
    json!({
        "recordings": hito.catalog.recordings(owner, Default::default()).await.unwrap(),
        "metadata": hito.catalog.recording_metadata(owner, recording, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(owner, recording).await.unwrap(),
        "playlists": hito.collection.playlists(owner).await.unwrap(),
        "playlist": hito.collection.playlist(owner, playlist).await.unwrap(),
        "activity": hito.collection.activity(owner).await.unwrap(),
    })
}

async fn connect(
    hito: &Services,
    owner: &Actor,
    server: &fixture::ProviderServer,
) -> PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            owner,
            OfferPluginService {
                name: "Owned revocation fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            owner,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap()
}
