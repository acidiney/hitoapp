#[path = "fixtures/content_service.rs"]
mod fixture;
mod support;

use hito_core::{AdoptProviderRecording, InstallPlugin, OfferPluginService};

async fn connect(
    hito: &hito_core::Services,
    actor: &hito_core::Actor,
    server: &fixture::ProviderServer,
) -> hito_core::PluginInstallation {
    let service = hito
        .plugins
        .offer_service(
            actor,
            OfferPluginService {
                name: "Artist fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    hito.plugins
        .install(
            actor,
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

#[tokio::test]
async fn saved_recording_without_an_album_or_source_has_an_artist_without_an_invented_release() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-artists").await;
    let installed = connect(&hito, &actor, &server).await;
    server
        .reply(
            "v1/recordings/detail",
            serde_json::json!({
                "id":"track", "title":"Unreleased recording", "artist":"Exact artist"
            }),
        )
        .await;
    server
        .reply("v1/sources", serde_json::json!({"items":[]}))
        .await;
    let recording = hito
        .catalog
        .adopt_provider(
            &actor,
            &installed.id,
            AdoptProviderRecording {
                installation_revision: installed.revision,
                provider_recording_id: "track".into(),
            },
        )
        .await
        .unwrap();
    assert!(!recording.available);
    assert!(recording.release_id.is_none());
    let artists = hito
        .catalog
        .artists(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 1);
    assert_eq!(artists.items[0].name, "Exact artist");
    assert_eq!(artists.items[0].release_count, 0);
    assert_eq!(
        serde_json::to_value(&artists.items[0]).unwrap()["recording_count"],
        1
    );
    let detail = hito
        .catalog
        .artist(&actor, &artists.items[0].id, Default::default())
        .await
        .unwrap();
    assert_eq!(detail.artist.id, artists.items[0].id);
    assert_eq!(detail.releases.total, 0);
    assert_eq!(
        serde_json::to_value(&detail).unwrap()["recordings"]["total"],
        1
    );
    assert_eq!(
        hito.catalog
            .releases(&actor, Default::default())
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn artist_recording_filter_uses_exact_credits_instead_of_matching_names() {
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, actor, _shared, _music) = support::library("provider-artist-recordings").await;
    let installed = connect(&hito, &actor, &server).await;
    server
        .reply("v1/sources", serde_json::json!({"items":[]}))
        .await;
    let mut saved = Vec::new();
    for artist in ["Echo", "echo"] {
        server
            .reply(
                "v1/recordings/detail",
                serde_json::json!({
                    "id":artist, "title":format!("{artist} recording"), "artist":artist
                }),
            )
            .await;
        saved.push(
            hito.catalog
                .adopt_provider(
                    &actor,
                    &installed.id,
                    AdoptProviderRecording {
                        installation_revision: installed.revision,
                        provider_recording_id: artist.into(),
                    },
                )
                .await
                .unwrap(),
        );
    }
    let artists = hito
        .catalog
        .artists(&actor, Default::default())
        .await
        .unwrap();
    assert_eq!(artists.total, 2);
    for (artist, recording) in ["Echo", "echo"].into_iter().zip(saved) {
        let credit = artists
            .items
            .iter()
            .find(|credit| credit.name == artist)
            .unwrap();
        let search: hito_core::Search =
            serde_json::from_value(serde_json::json!({"artist":credit.id})).unwrap();
        let tracks = hito.catalog.recordings(&actor, search).await.unwrap();
        assert_eq!(tracks.total, 1);
        assert_eq!(tracks.items[0].id, recording.id);
    }
}

#[tokio::test]
async fn private_artist_edits_keep_identity_through_refresh_and_unavailable_sources() {
    use hito_core::{Action, Credentials, Error, Search};
    let server = fixture::ProviderServer::start().await;
    let (_dir, hito, alice, _shared, _music) = support::library("private-provider-artists").await;
    let installed = connect(&hito, &alice, &server).await;
    server
        .reply(
            "v1/recordings/detail",
            serde_json::json!({"id":"track","title":"Saved track","artist":"Observed artist"}),
        )
        .await;
    server
        .reply(
            "v1/sources",
            serde_json::json!({"items":[{"id":"audio","label":"Original","ready":true}]}),
        )
        .await;
    let request = || AdoptProviderRecording {
        installation_revision: installed.revision,
        provider_recording_id: "track".into(),
    };
    let first = hito
        .catalog
        .adopt_provider(&alice, &installed.id, request())
        .await
        .unwrap();
    hito.identity
        .create_user(
            &alice,
            Credentials {
                username: "bob".into(),
                password: "private artist password".into(),
            },
        )
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "bob".into(),
            password: "private artist password".into(),
        })
        .await
        .unwrap();
    let bob = hito.identity.authenticate(&session.token).await.unwrap();
    let bob_connection = hito
        .plugins
        .install(
            &bob,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: installed.service_id.clone(),
                service_revision: 1,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let theirs = hito
        .catalog
        .adopt_provider(&bob, &bob_connection.id, request())
        .await
        .unwrap();
    let recording_id = uuid::Uuid::parse_str(&first.id).unwrap();
    support::synchronize(
        &hito,
        &alice,
        vec![Action::RecordingArtistSet {
            recording_id,
            artist: "Personal artist".into(),
        }],
    )
    .await;
    let personal = hito
        .catalog
        .artists(&alice, Default::default())
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(personal.name, "Personal artist");
    let detail = hito
        .catalog
        .artist(&alice, &personal.id, Default::default())
        .await
        .unwrap();
    assert_eq!(detail.recordings.items[0].id, first.id);
    assert_eq!(detail.recordings.items[0].artist, "Personal artist");
    assert!(matches!(
        hito.catalog
            .artist(&bob, &personal.id, Default::default())
            .await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        hito.catalog
            .recordings(
                &bob,
                Search {
                    artist: Some(personal.id.clone()),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .total,
        0
    );
    server
        .reply(
            "v1/recordings/detail",
            serde_json::json!({"id":"track","title":"Updated track","artist":"Personal artist"}),
        )
        .await;
    let updated = hito
        .catalog
        .adopt_provider(&alice, &installed.id, request())
        .await
        .unwrap();
    assert_eq!(updated.id, first.id);
    let current = hito
        .catalog
        .artists(&alice, Default::default())
        .await
        .unwrap();
    assert_eq!(current.total, 1);
    assert_eq!(current.items[0].id, personal.id);
    support::synchronize(
        &hito,
        &alice,
        vec![Action::RecordingArtistInherit { recording_id }],
    )
    .await;
    assert_eq!(
        hito.catalog
            .artists(&alice, Default::default())
            .await
            .unwrap()
            .items[0]
            .id,
        personal.id
    );
    hito.plugins
        .remove(&alice, &installed.id, installed.revision)
        .await
        .unwrap();
    let retained = hito
        .catalog
        .artist(&alice, &personal.id, Default::default())
        .await
        .unwrap();
    assert_eq!(retained.releases.total, 0);
    assert_eq!(retained.recordings.total, 1);
    assert!(!retained.recordings.items[0].available);
    assert_eq!(
        hito.catalog
            .recordings(&bob, Default::default())
            .await
            .unwrap()
            .items[0]
            .id,
        theirs.id
    );
    assert_eq!(
        hito.catalog
            .artists(&bob, Default::default())
            .await
            .unwrap()
            .items[0]
            .name,
        "Observed artist"
    );
}
