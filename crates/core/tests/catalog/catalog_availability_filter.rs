use hito_core::test_support::{playlist_service, provider_audio::AudioLibrary, support};
use hito_core::{
    Action, Actor, Credentials, ImportProviderPlaylist, InstallPlugin, OfferPluginService,
    RecordingAvailability, Search, Services,
};
use serde_json::json;
use std::collections::BTreeSet;
use uuid::Uuid;

type Combination = (bool, bool);

async fn page(
    hito: &Services,
    actor: &Actor,
    availability: Option<RecordingAvailability>,
    offset: i64,
    limit: i64,
) -> hito_core::Page<hito_core::Recording> {
    hito.catalog
        .recordings(
            actor,
            Search {
                availability,
                offset: Some(offset),
                limit: Some(limit),
                ..Search::default()
            },
        )
        .await
        .unwrap()
}

async fn paged_ids(
    hito: &Services,
    actor: &Actor,
    availability: Option<RecordingAvailability>,
) -> (i64, Vec<String>) {
    let first = page(hito, actor, availability, 0, 1).await;
    let mut ids: Vec<String> = first.items.iter().map(|item| item.id.clone()).collect();
    for offset in 1..first.total + 1 {
        let next = page(hito, actor, availability, offset, 1).await;
        assert_eq!(next.total, first.total, "Totals stay stable across pages");
        ids.extend(next.items.into_iter().map(|item| item.id));
    }
    (first.total, ids)
}

async fn assert_filters_partition(hito: &Services, actor: &Actor) -> BTreeSet<Combination> {
    let all = page(hito, actor, None, 0, 200).await;
    let ordered: Vec<String> = all.items.iter().map(|item| item.id.clone()).collect();
    let combinations = all
        .items
        .iter()
        .map(|item| (item.available, item.plays_from_services))
        .collect();
    let expected = |wanted: bool| -> Vec<String> {
        all.items
            .iter()
            .filter(|item| (item.available || item.plays_from_services) == wanted)
            .map(|item| item.id.clone())
            .collect()
    };
    for (availability, wanted) in [
        (RecordingAvailability::AvailableOrService, true),
        (RecordingAvailability::NoSource, false),
    ] {
        let (total, ids) = paged_ids(hito, actor, Some(availability)).await;
        assert_eq!(
            ids,
            expected(wanted),
            "{availability:?} keeps the default order"
        );
        assert_eq!(
            total,
            ids.len() as i64,
            "{availability:?} total counts the filtered rows"
        );
    }
    let (total, ids) = paged_ids(hito, actor, None).await;
    assert_eq!(ids, ordered);
    assert_eq!(total, all.total);
    combinations
}

#[tokio::test]
async fn availability_filters_partition_recordings_by_source_and_service_state() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let library = f
        .hito
        .catalog
        .libraries(&f.actor)
        .await
        .unwrap()
        .remove(0)
        .id;
    support::scan(&f.hito, &f.actor, &library).await;

    let playlists = playlist_service::ProviderServer::start().await;
    let installed =
        playlist_service::connect(&f.hito, &f.actor, &playlists, &["playlists", "sources"]).await;
    playlist_service::playlist(
        &playlists,
        "playlist/filter",
        "v1",
        vec![
            json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
            "id": "bound", "title": "Bound", "artist": "Band", "duration_ms": 100000,
            "release": {"id": "album-bound", "title": "Album"}, "disc_number": 1, "track_number": 1,
            "revision": "v1"}}}),
        ],
    )
    .await;
    f.hito
        .collection
        .import_provider_playlist(
            &f.actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/filter".into(),
            },
        )
        .await
        .unwrap();

    let mut seen = assert_filters_partition(&f.hito, &f.actor).await;
    f.hito
        .plugins
        .remove(&f.actor, &installed.id, installed.revision)
        .await
        .unwrap();
    seen.extend(assert_filters_partition(&f.hito, &f.actor).await);
    assert_eq!(
        seen,
        BTreeSet::from([(false, false), (false, true), (true, false), (true, true)]),
        "The fixture covers every combination of the two row flags"
    );
}

#[tokio::test]
async fn availability_filters_combine_with_search_and_likes() {
    let (_directory, hito, actor, library, music) = support::library("availability-search").await;
    support::write_wav(&music.join("local.wav"), 3);
    support::scan(&hito, &actor, &library).await;
    let liked = Search {
        liked: Some(true),
        availability: Some(RecordingAvailability::AvailableOrService),
        ..Search::default()
    };
    assert_eq!(
        hito.catalog.recordings(&actor, liked).await.unwrap().total,
        0
    );
    let searched = |availability| Search {
        q: Some("local".into()),
        availability: Some(availability),
        ..Search::default()
    };
    assert_eq!(
        hito.catalog
            .recordings(&actor, searched(RecordingAvailability::AvailableOrService))
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        hito.catalog
            .recordings(&actor, searched(RecordingAvailability::NoSource))
            .await
            .unwrap()
            .total,
        0
    );
}

async fn guest_of(hito: &Services, admin: &Actor, library: &str) -> Actor {
    let credentials = || Credentials {
        username: "guest".into(),
        password: "availability guest password".into(),
    };
    let guest = hito
        .identity
        .create_user(admin, credentials())
        .await
        .unwrap();
    hito.identity
        .set_library_access(admin, library, &guest.id, true)
        .await
        .unwrap();
    let session = hito.identity.login(credentials()).await.unwrap();
    hito.identity.authenticate(&session.token).await.unwrap()
}

async fn filtered(hito: &Services, actor: &Actor) -> [(i64, Vec<String>); 2] {
    [
        paged_ids(hito, actor, Some(RecordingAvailability::AvailableOrService)).await,
        paged_ids(hito, actor, Some(RecordingAvailability::NoSource)).await,
    ]
}

#[tokio::test]
async fn another_accounts_service_binding_does_not_change_the_filter() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let libraries = f.hito.catalog.libraries(&f.actor).await.unwrap();
    let shared = libraries.iter().find(|library| !library.personal).unwrap();
    support::scan(&f.hito, &f.actor, &shared.id).await;
    let admin_before = filtered(&f.hito, &f.actor).await;
    assert!(admin_before[0].1.contains(&f.source.recording_id));

    let guest = guest_of(&f.hito, &f.actor, &shared.id).await;
    let server = playlist_service::ProviderServer::start().await;
    server
        .reply(
            "v1/manifest",
            json!({"id": "org.hito.guest-fixture", "name": "Guest fixture",
                "protocol_version": 1, "capabilities": ["playlists", "sources"]}),
        )
        .await;
    let offered = f
        .hito
        .plugins
        .offer_service(
            &f.actor,
            OfferPluginService {
                name: "Guest fixture".into(),
                base_url: server.url.clone(),
            },
        )
        .await
        .unwrap();
    let installed = f
        .hito
        .plugins
        .install(
            &guest,
            InstallPlugin {
                accept_manifest_digest: None,
                service_id: offered.id,
                service_revision: offered.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    playlist_service::playlist(
        &server,
        "playlist/guest",
        "v1",
        vec![
            json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
            "id": "guest", "title": "Guest", "artist": "Band", "duration_ms": 100000,
            "release": {"id": "album-guest", "title": "Album"}, "disc_number": 1, "track_number": 1,
            "revision": "v1"}}}),
        ],
    )
    .await;
    f.hito
        .collection
        .import_provider_playlist(
            &guest,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/guest".into(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        filtered(&f.hito, &f.actor).await,
        admin_before,
        "Another account's service and imports leave both filters unchanged"
    );
    let combinations = assert_filters_partition(&f.hito, &guest).await;
    assert!(
        combinations.contains(&(false, true)),
        "The guest's own binding counts for the guest"
    );
    let guest_lists = filtered(&f.hito, &guest).await;
    assert!(
        !guest_lists[0].1.contains(&f.source.recording_id)
            && !guest_lists[1].1.contains(&f.source.recording_id),
        "The admin's personal provider recording never reaches the guest"
    );
}

#[tokio::test]
async fn availability_filters_keep_membership_counts_and_pages_after_a_personal_edit() {
    let f = AudioLibrary::start().await;
    f.ready().await;
    let libraries = f.hito.catalog.libraries(&f.actor).await.unwrap();
    let shared = libraries
        .iter()
        .find(|library| !library.personal)
        .unwrap()
        .id
        .clone();
    support::scan(&f.hito, &f.actor, &shared).await;
    let playlists = playlist_service::ProviderServer::start().await;
    let installed =
        playlist_service::connect(&f.hito, &f.actor, &playlists, &["playlists", "sources"]).await;
    playlist_service::playlist(
        &playlists,
        "playlist/personal",
        "v1",
        vec![
            json!({"occurrence_id": "one", "item": {"kind": "recording", "recording": {
            "id": "unbound", "title": "Unbound", "artist": "Band", "duration_ms": 100000,
            "release": {"id": "album-unbound", "title": "Album"}, "disc_number": 1, "track_number": 1,
            "revision": "v1"}}}),
        ],
    )
    .await;
    f.hito
        .collection
        .import_provider_playlist(
            &f.actor,
            &installed.id,
            ImportProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installed.revision,
                playlist_reference: "https://provider.invalid/playlist/personal".into(),
            },
        )
        .await
        .unwrap();
    f.hito
        .plugins
        .remove(&f.actor, &installed.id, installed.revision)
        .await
        .unwrap();
    let guest = guest_of(&f.hito, &f.actor, &shared).await;
    let admin_before = filtered(&f.hito, &f.actor).await;
    let guest_before = filtered(&f.hito, &guest).await;
    assert!(
        !admin_before[0].1.is_empty() && !admin_before[1].1.is_empty(),
        "both filters have members before the edit"
    );

    let everything = page(&f.hito, &f.actor, None, 0, 200).await;
    support::synchronize(
        &f.hito,
        &f.actor,
        everything
            .items
            .iter()
            .map(|item| Action::RecordingTitleSet {
                recording_id: Uuid::parse_str(&item.id).unwrap(),
                title: format!("Personal {}", item.title),
            })
            .collect(),
    )
    .await;

    assert_filters_partition(&f.hito, &f.actor).await;
    assert_filters_partition(&f.hito, &guest).await;
    assert_eq!(filtered(&f.hito, &f.actor).await, admin_before);
    assert_eq!(filtered(&f.hito, &guest).await, guest_before);
    for availability in [
        RecordingAvailability::AvailableOrService,
        RecordingAvailability::NoSource,
    ] {
        let edited = page(&f.hito, &f.actor, Some(availability), 0, 200).await;
        assert!(
            edited
                .items
                .iter()
                .all(|item| item.title.starts_with("Personal ")),
            "{availability:?} returns the account's personal titles"
        );
        let unedited = page(&f.hito, &guest, Some(availability), 0, 200).await;
        assert!(
            unedited
                .items
                .iter()
                .all(|item| !item.title.starts_with("Personal ")),
            "{availability:?} never shows another account's personal titles"
        );
    }
    let personal = f
        .hito
        .catalog
        .recordings(
            &f.actor,
            Search {
                q: Some("Personal Unbound".into()),
                availability: Some(RecordingAvailability::NoSource),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(personal.total, 1);
    assert_eq!(personal.items[0].title, "Personal Unbound");
}
