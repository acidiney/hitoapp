use super::{
    catalog_fixture::{install, snapshot},
    fixture::{RemoteServer, local},
};
use hito_core::BeginCatalogSnapshot;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn rejected_replacements_and_interrupted_staging_preserve_the_committed_catalog() {
    let (directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let old = Uuid::new_v4();
    let (metadata, pages) = snapshot(old, &["old"]);
    remote.catalog(metadata.clone(), pages.clone());
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    install(&services, &actor, &session).await.unwrap();
    for case in 0..25 {
        let (mut metadata, mut pages) = snapshot(Uuid::new_v4(), &["new-a", "new-b"]);
        match case {
            0 => metadata["account_id"] = json!("other"),
            1 => metadata["library_id"] = json!("other"),
            2 => metadata["recording_count"] = json!(100001),
            3 => metadata["page_count"] = json!(0),
            4 => metadata["page_count"] = json!(3),
            5 => metadata["collection_watermark"] = json!(-1),
            6 => pages[0]["snapshot_id"] = json!(Uuid::new_v4()),
            7 => pages[0]["ordinal"] = json!(1),
            8 => pages[0]["next_ordinal"] = Value::Null,
            9 => pages[0]["recordings"] = json!([]),
            10 => pages[1]["recordings"][0]["id"] = json!("new-a"),
            11 => pages[1]["recordings"][0]["sources"][0]["id"] = json!("source-0"),
            12 => {
                pages[1]["recordings"][0]["sources"][0]["release"]["title"] =
                    json!("Conflicting album")
            }
            13 => pages[0]["recordings"][0]["library_id"] = json!("other"),
            14 => pages[0]["recordings"][0]["duration_ms"] = json!(-1),
            15 => pages[0]["recordings"][0]["sources"][0]["duration_ms"] = json!(-1),
            16 => pages[0]["recordings"][0]["sources"][0]["release"]["disc_number"] = json!(0),
            17 => pages[0]["recordings"][0]["sources"][0]["release"]["track_number"] = json!(-1),
            18 => pages[0]["recordings"][0]["sources"] = json!([]),
            19 => pages[0]["recordings"][0]["title"] = json!("x".repeat(65536)),
            20 => pages[1]["next_ordinal"] = json!(2),
            21 => {
                metadata["recording_count"] = json!(3);
            }
            22 => pages[1]["recordings"][0]["sources"][0]["release"]["entry_id"] = json!("entry-0"),
            23 => {
                let source = pages[0]["recordings"][0]["sources"][0].clone();
                pages[0]["recordings"][0]["sources"] = Value::Array(
                    (0..129)
                        .map(|ordinal| {
                            let mut source = source.clone();
                            source["id"] = json!(format!("source-{ordinal}"));
                            source
                        })
                        .collect(),
                );
            }
            24 => {
                metadata["recording_count"] = json!(201);
                metadata["page_count"] = json!(1);
                pages[0]["next_ordinal"] = Value::Null;
                let fact = pages[0]["recordings"][0].clone();
                pages[0]["recordings"] = Value::Array(
                    (0..201)
                        .map(|ordinal| {
                            let mut fact = fact.clone();
                            fact["id"] = json!(format!("recording-{ordinal}"));
                            fact["sources"][0]["id"] = json!(format!("source-{ordinal}"));
                            fact
                        })
                        .collect(),
                );
            }
            _ => unreachable!(),
        }
        remote.catalog(metadata, pages);
        assert!(
            install(&services, &actor, &session).await.is_err(),
            "malformed case {case} was published"
        );
        assert_eq!(
            services
                .replicas
                .catalog_snapshot(&actor, &session, "remote-library")
                .await
                .unwrap()
                .unwrap()
                .id,
            old,
            "malformed case {case} replaced the cache"
        );
    }
    let (metadata, mut incomplete) = snapshot(Uuid::new_v4(), &["staged-a", "staged-b"]);
    incomplete.pop();
    remote.catalog(metadata, incomplete);
    assert!(install(&services, &actor, &session).await.is_err());
    services.replicas.close().await.unwrap();
    drop(services);
    let (services, actor) = super::durable::reopen(directory.path()).await;
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "remote-library")
            .await
            .unwrap()
            .unwrap()
            .id,
        old
    );
    let next = Uuid::new_v4();
    let (metadata, pages) = snapshot(next, &[]);
    remote.catalog(metadata, pages);
    install(&services, &actor, &session).await.unwrap();
    assert!(
        services
            .replicas
            .catalog_page(&actor, &session, next, 0)
            .await
            .unwrap()
            .recordings
            .is_empty()
    );
    assert!(
        services
            .replicas
            .catalog_page(&actor, &session, old, 0)
            .await
            .is_err()
    );
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn a_snapshot_identity_cannot_alias_two_cached_libraries() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
    let id = Uuid::new_v4();
    let (metadata, pages) = snapshot(id, &["original"]);
    remote.catalog(metadata, pages);
    let connection = services
        .replicas
        .connect(&actor, remote.request())
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    install(&services, &actor, &session).await.unwrap();
    let (mut metadata, mut pages) = snapshot(id, &["other"]);
    metadata["library_id"] = json!("other-library");
    pages[0]["recordings"][0]["library_id"] = json!("other-library");
    remote.catalog(metadata, pages);
    assert!(
        services
            .replicas
            .install_catalog(
                &actor,
                &session,
                BeginCatalogSnapshot {
                    library_id: "other-library".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        services
            .replicas
            .catalog_page(&actor, &session, id, 0)
            .await
            .unwrap()
            .recordings[0]
            .id,
        "original"
    );
    assert!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "other-library")
            .await
            .unwrap()
            .is_none()
    );
    services.replicas.close().await.unwrap();
}
