use hito_core::Credentials;
use hito_server::RuntimeConfig;

#[tokio::test]
async fn installation_manifest_loads_multiple_grants_without_exposing_host_paths() {
    let directory = tempfile::tempdir().unwrap();
    let web = directory.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("assets/app.js"), "export {};").unwrap();
    let manifest = directory.path().join("storage.json");
    let grants: Vec<_> = ["Music", "Archive"]
        .map(|name| {
            let root = directory.path().join(name);
            std::fs::create_dir(&root).unwrap();
            serde_json::json!({"id": name.to_lowercase(), "name": name, "root": root})
        })
        .into();
    std::fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({"version": 1, "directories": grants})).unwrap(),
    )
    .unwrap();
    let config =
        RuntimeConfig::load_with_manifest(directory.path().join("hito.db"), manifest, web, None)
            .await
            .unwrap();
    let services = hito_core::open(config.core).await.unwrap();
    let session = services
        .identity
        .setup(Credentials {
            username: "admin".into(),
            password: "manifest fixture password".into(),
        })
        .await
        .unwrap();
    let actor = services
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let directories = services.storage.shared_directories(&actor).await.unwrap();
    assert_eq!(directories.len(), 2);
    assert!(directories.iter().all(|grant| grant.available));
    let public = serde_json::to_string(&directories).unwrap();
    assert!(!public.contains(directory.path().to_str().unwrap()));
}
