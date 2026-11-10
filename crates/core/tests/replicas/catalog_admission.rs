use super::{
    catalog_fixture::{install, snapshot},
    fixture::{RemoteServer, local},
};
use hito_core::BeginCatalogSnapshot;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn catalog_admission_bounds_libraries_and_preserves_heads_when_full() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
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
    let mut ids = Vec::new();
    for ordinal in 0..17 {
        let library = format!("library-{ordinal}");
        let id = Uuid::new_v4();
        let (mut metadata, mut pages) = snapshot(id, &["recording"]);
        metadata["library_id"] = json!(library);
        pages[0]["recordings"][0]["library_id"] = json!(library);
        remote.catalog(metadata, pages);
        let result = services
            .replicas
            .install_catalog(
                &actor,
                &session,
                BeginCatalogSnapshot {
                    library_id: library.clone(),
                },
            )
            .await;
        if ordinal < 16 {
            result.unwrap();
            ids.push((library, id));
        } else {
            assert!(matches!(result, Err(hito_core::Error::Conflict(_))));
            assert!(
                services
                    .replicas
                    .catalog_snapshot(&actor, &session, &library)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }
    for (library, id) in ids {
        assert_eq!(
            services
                .replicas
                .catalog_snapshot(&actor, &session, &library)
                .await
                .unwrap()
                .unwrap()
                .id,
            id
        );
    }
    services.replicas.close().await.unwrap();
}

#[tokio::test]
async fn wire_bytes_bound_exports_and_old_plus_staging_profile_storage() {
    let (_directory, services, actor) = local().await;
    let remote = RemoteServer::start().await;
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
    remote.pad_catalog_pages(256 * 1024);
    let names = (0..513)
        .map(|ordinal| format!("recording-{ordinal}"))
        .collect::<Vec<_>>();
    let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
    let (metadata, pages) = snapshot(Uuid::new_v4(), &refs);
    remote.catalog(metadata, pages);
    assert!(install(&services, &actor, &session).await.is_err());
    assert!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "remote-library")
            .await
            .unwrap()
            .is_none()
    );
    let first = Uuid::new_v4();
    let (metadata, pages) = snapshot(first, &refs[..512]);
    remote.catalog(metadata, pages);
    install(&services, &actor, &session).await.unwrap();
    let second = Uuid::new_v4();
    let (mut metadata, mut pages) = snapshot(second, &refs[..512]);
    metadata["library_id"] = json!("other-library");
    for page in &mut pages {
        page["recordings"][0]["library_id"] = json!("other-library");
    }
    remote.catalog(metadata, pages);
    services
        .replicas
        .install_catalog(
            &actor,
            &session,
            BeginCatalogSnapshot {
                library_id: "other-library".into(),
            },
        )
        .await
        .unwrap();
    let (metadata, pages) = snapshot(Uuid::new_v4(), &["replacement"]);
    remote.catalog(metadata, pages);
    assert!(matches!(
        install(&services, &actor, &session).await,
        Err(hito_core::Error::Conflict(_))
    ));
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "remote-library")
            .await
            .unwrap()
            .unwrap()
            .id,
        first
    );
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "other-library")
            .await
            .unwrap()
            .unwrap()
            .id,
        second
    );
    services.replicas.close().await.unwrap();
}
