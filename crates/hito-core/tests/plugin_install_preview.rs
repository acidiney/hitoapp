#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use futures_util::FutureExt;
use hito_core::{
    Action, Actor, Error, InstallPlugin, OfferPluginService, PluginRefresh, RefreshPlugin, Services,
};
use serde_json::{Value, json};
use std::{future::pending, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

const TOKEN: &str = "private-install-preview-token-sentinel";
const NAME: &str = "private-install-preview-provider-sentinel";

#[tokio::test]
async fn an_explicit_download_declaration_can_be_previewed_and_accepted_exactly() {
    workflow(Some("explicit_download")).await;
}

#[tokio::test]
async fn a_playback_only_declaration_can_be_previewed_and_accepted_exactly() {
    workflow(Some("playback_only")).await;
}

#[tokio::test]
async fn an_undeclared_manifest_can_be_previewed_without_losing_legacy_installation() {
    workflow(None).await;
}

async fn workflow(persistence: Option<&str>) {
    let (directory, hito, actor, _library) =
        tokio::time::timeout(Duration::from_secs(10), support::catalog())
            .await
            .expect("the owned catalog must open within its deadline");
    let original_path = directory.path().join("music/Alpha.wav");
    let original = std::fs::read(&original_path).unwrap();
    let server = fixture::ProviderServer::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        AssertUnwindSafe(exercise(&hito, &actor, &server, persistence)).catch_unwind(),
    )
    .await;
    let settled =
        tokio::time::timeout(Duration::from_secs(5), hito.downloads.finish_downloads()).await;
    let stopped = tokio::time::timeout(Duration::from_secs(2), server.shutdown()).await;
    assert!(
        matches!(settled, Ok(Ok(()))),
        "managed filesystem work must settle on success or panic"
    );
    assert!(stopped.is_ok(), "the owned provider must stop and join");
    assert!(
        std::fs::read(original_path).unwrap() == original,
        "preview and acceptance must preserve owned original audio"
    );
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the public installation preview workflow exceeded its deadline"),
    }
}

async fn exercise(
    hito: &Services,
    actor: &Actor,
    server: &fixture::ProviderServer,
    persistence: Option<&str>,
) {
    let recording = hito
        .catalog
        .recordings(actor, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    let recording_id = Uuid::parse_str(&recording.id).unwrap();
    support::synchronize(
        hito,
        actor,
        vec![
            Action::RecordingTitleSet {
                recording_id,
                title: "private-install-preview-title-sentinel".into(),
            },
            Action::LikeSet {
                recording_id,
                liked: true,
            },
            Action::ProgressSet {
                recording_id,
                position_ms: 650,
                speed: 1.25,
            },
        ],
    )
    .await;
    let preserved = private_state(hito, actor, &recording.id).await;
    let mut manifest = json!({
        "id": "org.hito.install-preview-fixture",
        "name": NAME,
        "protocol_version": 1,
        "capabilities": ["sources", "search"]
    });
    if let Some(persistence) = persistence {
        manifest["content_policy"] = json!({"source_persistence": persistence});
    }
    server.reply("v1/manifest", manifest.clone()).await;
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Owned installation preview fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let draft = || {
        json!({
            "service_id": service.id,
            "service_revision": service.revision,
            "bearer_token": TOKEN
        })
    };
    let request = |digest: Option<&str>| {
        let mut value = draft();
        value["accept_manifest_digest"] = json!(digest);
        serde_json::from_value::<InstallPlugin>(value).unwrap()
    };
    let preview = hito
        .plugins
        .preview_installation(actor, serde_json::from_value(draft()).unwrap())
        .await
        .unwrap();
    assert_eq!(preview.digest.len(), 64);
    assert!(preview.digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let encoded = serde_json::to_value(&preview).unwrap();
    assert!(!encoded.to_string().contains(TOKEN));
    assert!(encoded.get("credential").is_none() && encoded.get("credential_set").is_none());
    assert_uninstalled(hito, actor).await;
    assert!(private_state(hito, actor, &recording.id).await == preserved);

    manifest["capabilities"] = json!(["search", "sources"]);
    server.reply("v1/manifest", manifest.clone()).await;
    let equivalent = hito
        .plugins
        .preview_installation(actor, serde_json::from_value(draft()).unwrap())
        .await
        .unwrap();
    assert_eq!(equivalent.digest, preview.digest);
    assert_uninstalled(hito, actor).await;
    assert_eq!(server.requests("v1/manifest").await.len(), 2);

    let mut changed = manifest.clone();
    if persistence.is_some() {
        changed.as_object_mut().unwrap().remove("content_policy");
    } else {
        changed["name"] = "Changed declaration".into();
    }
    server.reply("v1/manifest", changed).await;
    let rejected = hito
        .plugins
        .install(actor, request(Some(&preview.digest)))
        .await;
    let message = match rejected {
        Err(Error::Conflict(message)) => message,
        Err(error) => panic!(
            "expected an exact-review conflict, received {}",
            error.code()
        ),
        Ok(_) => panic!("acceptance must refetch and compare even if policy disappears"),
    };
    assert!(message.len() <= 256 && message.to_ascii_lowercase().contains("review"));
    for private in [TOKEN, NAME, &server.url, preview.digest.as_str()] {
        assert!(
            !message.contains(private),
            "review conflicts must omit private inputs"
        );
    }
    assert_uninstalled(hito, actor).await;
    assert!(private_state(hito, actor, &recording.id).await == preserved);

    server.reply("v1/manifest", manifest.clone()).await;
    let installed = hito
        .plugins
        .install(actor, request(Some(&preview.digest)))
        .await
        .unwrap();
    assert!(installed.credential_set && installed.revision == 1);
    assert!(
        serde_json::to_value(&installed.manifest).unwrap()
            == serde_json::to_value(&preview.manifest).unwrap()
    );
    assert_eq!(server.requests("v1/manifest").await.len(), 4);

    manifest["name"] = "Changed declaration for refresh comparison".into();
    server.reply("v1/manifest", manifest).await;
    let PluginRefresh::ReviewRequired { manifest, digest } = hito
        .plugins
        .refresh(
            actor,
            &installed.id,
            RefreshPlugin {
                revision: installed.revision,
                accept_manifest_digest: None,
                credential: Default::default(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("the changed declaration must require refresh review");
    };
    hito.plugins
        .remove(actor, &installed.id, installed.revision)
        .await
        .unwrap();
    let next_preview = hito
        .plugins
        .preview_installation(actor, serde_json::from_value(draft()).unwrap())
        .await
        .unwrap();
    assert_eq!(next_preview.digest, digest);
    assert!(
        serde_json::to_value(next_preview.manifest).unwrap()
            == serde_json::to_value(manifest).unwrap()
    );
    assert_uninstalled(hito, actor).await;
    if persistence.is_none() {
        let legacy = hito.plugins.install(actor, request(None)).await.unwrap();
        assert!(legacy.manifest.content_policy.is_none());
        hito.plugins
            .remove(actor, &legacy.id, legacy.revision)
            .await
            .unwrap();
    }
    assert_uninstalled(hito, actor).await;
    assert!(private_state(hito, actor, &recording.id).await == preserved);
    assert!(matches!(
        hito.downloads.status(actor, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(!hito.downloads.run_next_job_until(pending()).await.unwrap());
    assert_eq!(
        server.requests("v1/manifest").await.len(),
        if persistence.is_none() { 7 } else { 6 }
    );
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/recordings/metadata",
        "v1/sources",
        "v1/playlists/detail",
        "v1/playlists/items",
        "v1/audio",
    ] {
        assert!(
            server.requests(path).await.is_empty(),
            "preview and acceptance must request only manifests"
        );
    }
}

async fn assert_uninstalled(hito: &Services, actor: &Actor) {
    assert!(
        hito.plugins
            .settings(actor)
            .await
            .unwrap()
            .installations
            .is_empty(),
        "preview or a rejected acceptance must save no active installation or credential"
    );
}

async fn private_state(hito: &Services, actor: &Actor, recording: &str) -> Value {
    json!({
        "catalog": hito.catalog.recordings(actor, Default::default()).await.unwrap(),
        "sources": hito.catalog.sources(actor, recording).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap()
    })
}
