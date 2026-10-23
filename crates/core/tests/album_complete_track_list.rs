use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::test_support::support;

use hito_core::{
    Action, Actor, AdoptProviderRecording, AlbumEditionTrackListAvailability, ArtistAssociation,
    ArtistCreditRole, AssociateRecordingArtist, Browse, Credentials, Error, InstallPlugin,
    MetadataProvider, OfferPluginService, PluginInstallation, ReadAlbumEditionTrackList, Search,
    Services, UpdateMetadataSettings,
};
use serde_json::{Value, json};
use uuid::Uuid;

struct Album {
    hito: Services,
    actor: Actor,
    installation: PluginInstallation,
    association: ArtistAssociation,
    release_id: String,
    first: String,
    second: String,
    extra: String,
    _directory: tempfile::TempDir,
}

const PERSONAL_TITLE: &str = "Private personal title";

impl Album {
    async fn prepare(server: &AlbumProvider) -> Self {
        let (directory, hito, actor, _library, _music) = support::library("album-complete").await;
        let service = hito
            .plugins
            .offer_service(
                &actor,
                OfferPluginService {
                    name: "Owned album editions".into(),
                    base_url: server.url.clone(),
                },
            )
            .await
            .unwrap();
        let installation = hito
            .plugins
            .install(
                &actor,
                InstallPlugin {
                    accept_manifest_digest: None,
                    service_id: service.id,
                    service_revision: service.revision,
                    bearer_token: None,
                },
            )
            .await
            .unwrap();
        let mut saved = Vec::new();
        for id in [provider::FIRST, provider::SECOND, provider::EXTRA] {
            saved.push(
                hito.catalog
                    .adopt_provider(
                        &actor,
                        &installation.id,
                        AdoptProviderRecording {
                            installation_revision: installation.revision,
                            provider_recording_id: id.into(),
                        },
                    )
                    .await
                    .unwrap(),
            );
        }
        let release_id = saved[0].release_id.clone().expect("a provider release");
        assert!(
            saved
                .iter()
                .all(|r| r.release_id.as_ref() == Some(&release_id))
        );
        support::synchronize(
            &hito,
            &actor,
            vec![Action::RecordingTitleSet {
                recording_id: Uuid::parse_str(&saved[1].id).unwrap(),
                title: PERSONAL_TITLE.into(),
            }],
        )
        .await;
        let settings = hito.catalog.metadata_settings(&actor).await.unwrap();
        let selected = MetadataProvider::HttpPlugin {
            installation_id: installation.id.clone(),
        };
        let settings = hito
            .catalog
            .set_metadata_settings(
                &actor,
                UpdateMetadataSettings {
                    revision: settings.revision,
                    enabled: true,
                    providers: vec![selected.clone()],
                },
            )
            .await
            .unwrap();
        let artists = hito
            .catalog
            .artists(&actor, Browse::default())
            .await
            .unwrap();
        assert_eq!(artists.total, 1);
        let metadata = hito
            .catalog
            .recording_metadata(&actor, &saved[0].id, Default::default())
            .await
            .unwrap();
        let association = hito
            .catalog
            .associate_recording_artist(
                &actor,
                &saved[0].id,
                AssociateRecordingArtist {
                    artist_id: artists.items[0].id.clone(),
                    source_id: None,
                    observation_id: metadata.selected_observation.unwrap().id,
                    role: ArtistCreditRole::RecordingArtist,
                    provider: selected,
                    provider_artist_id: provider::ARTIST_ID.into(),
                    policy_revision: settings.revision,
                },
            )
            .await
            .unwrap();
        Self {
            hito,
            actor,
            installation,
            association,
            release_id,
            first: saved[0].id.clone(),
            second: saved[1].id.clone(),
            extra: saved[2].id.clone(),
            _directory: directory,
        }
    }

    fn request(&self, edition: &str) -> ReadAlbumEditionTrackList {
        ReadAlbumEditionTrackList {
            recording_id: self.first.clone(),
            association_id: self.association.id.clone(),
            release_group_id: provider::GROUP_ID.into(),
            edition_id: edition.into(),
        }
    }

    async fn read(&self, edition: &str) -> hito_core::Result<AlbumEditionTrackListAvailability> {
        self.hito
            .catalog
            .album_edition_track_list_until(
                &self.actor,
                &self.release_id,
                self.request(edition),
                std::future::pending(),
            )
            .await
    }

    async fn library(&self) -> Value {
        json!({
            "recordings": self.hito.catalog.recordings(&self.actor, Search::default()).await.unwrap(),
            "release": self.hito.catalog.release(&self.actor, &self.release_id, Browse::default()).await.unwrap(),
        })
    }
}

#[tokio::test]
async fn a_selected_edition_merges_with_the_library_only_by_exact_provider_identity() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let before = album.library().await;
    let details_before = server.requests_to("v1/recordings/detail").len();

    let AlbumEditionTrackListAvailability::Available {
        association_id,
        edition,
        matches,
        library_only,
        adoption,
        is_album_release,
    } = album.read(provider::EDITION_ID).await.unwrap()
    else {
        panic!("An exact selected edition must be available");
    };
    assert_eq!(association_id, album.association.id);
    assert!(
        is_album_release,
        "The deluxe edition is the saved provider release"
    );
    assert_eq!(edition.media.len(), 2);
    let order: Vec<&str> = edition
        .media
        .iter()
        .flat_map(|medium| medium.tracks.iter().map(|t| t.id.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            "occurrence:album/1/0",
            "occurrence:album/1/1",
            "occurrence:album/1/2",
            "occurrence:album/1/3",
            "occurrence:album/2/1",
            "occurrence:album/2/2",
            "occurrence:album/2/3",
        ]
    );

    let linked: Vec<(&str, &str)> = matches
        .iter()
        .map(|m| (m.occurrence_id.as_str(), m.recording.id.as_str()))
        .collect();
    assert_eq!(
        linked,
        [
            ("occurrence:album/1/1", album.first.as_str()),
            ("occurrence:album/1/2", album.second.as_str()),
            ("occurrence:album/2/1", album.first.as_str()),
        ],
        "Only exact provider recording identities link; the same-title twin, the bonus, the pregap and the unidentified track stay catalog-only"
    );
    let second = &matches[1].recording;
    assert_eq!(second.title, PERSONAL_TITLE, "Personal edits survive");
    assert_eq!(second.inherited_title, "Second provider title");

    assert_eq!(library_only.len(), 1);
    assert_eq!(library_only[0].recording.id, album.extra);
    assert_eq!(library_only[0].disc_number, Some(1));
    assert_eq!(library_only[0].track_number, Some(9));

    let adoption = adoption.expect("A provider that can save recordings offers playback");
    assert_eq!(adoption.installation_id, album.installation.id);
    assert_eq!(adoption.installation_revision, album.installation.revision);

    assert_eq!(
        server.requests_to("v1/recordings/detail").len(),
        details_before,
        "Reading must not adopt the catalog-only tracks"
    );
    assert!(server.requests_to("v1/sources").is_empty());
    assert!(server.requests_to("v1/audio").is_empty());
    assert_eq!(
        server.requests_to("v1/catalog/editions/track-list").len(),
        1
    );
    assert_eq!(
        album.library().await,
        before,
        "A read never changes recordings, releases or personal edits"
    );

    let AlbumEditionTrackListAvailability::Available {
        matches,
        library_only,
        is_album_release,
        ..
    } = album.read(provider::STANDARD_ID).await.unwrap()
    else {
        panic!("A partial edition is still available");
    };
    assert!(
        !is_album_release,
        "Another edition is not the saved release"
    );
    assert_eq!(matches.len(), 1);
    assert_eq!(
        library_only.len(),
        2,
        "Album recordings absent from the edition remain visible"
    );
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn provider_outage_and_missing_support_leave_the_library_untouched() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let before = album.library().await;

    server.set_outage(true);
    assert!(matches!(
        album.read(provider::EDITION_ID).await.unwrap(),
        AlbumEditionTrackListAvailability::Unavailable
    ));
    assert_eq!(album.library().await, before);

    server.set_outage(false);
    server.set_editions(false);
    assert!(matches!(
        album.read(provider::EDITION_ID).await.unwrap(),
        AlbumEditionTrackListAvailability::Unsupported
    ));

    let cancelled = album
        .hito
        .catalog
        .album_edition_track_list_until(
            &album.actor,
            &album.release_id,
            album.request(provider::EDITION_ID),
            std::future::ready(()),
        )
        .await
        .unwrap();
    assert!(matches!(
        cancelled,
        AlbumEditionTrackListAvailability::Cancelled
    ));

    let settings = album
        .hito
        .catalog
        .metadata_settings(&album.actor)
        .await
        .unwrap();
    album
        .hito
        .catalog
        .set_metadata_settings(
            &album.actor,
            UpdateMetadataSettings {
                revision: settings.revision,
                enabled: false,
                providers: settings.providers,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        album.read(provider::EDITION_ID).await.unwrap(),
        AlbumEditionTrackListAvailability::Disabled
    ));
    assert_eq!(album.library().await, before);
    assert_eq!(server.unexpected(), 0);
    assert!(server.finish().await);
}

#[tokio::test]
async fn other_accounts_and_unrelated_recordings_cannot_use_the_album_authority() {
    let mut server = AlbumProvider::start().await;
    let album = Album::prepare(&server).await;
    let credentials = Credentials {
        username: "stranger".into(),
        password: "album stranger password".into(),
    };
    album
        .hito
        .identity
        .create_user(
            &album.actor,
            Credentials {
                username: credentials.username.clone(),
                password: credentials.password.clone(),
            },
        )
        .await
        .unwrap();
    let session = album.hito.identity.login(credentials).await.unwrap();
    let stranger = album
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let requests_before = server.requests().len();
    let denied = album
        .hito
        .catalog
        .album_edition_track_list_until(
            &stranger,
            &album.release_id,
            album.request(provider::EDITION_ID),
            std::future::pending(),
        )
        .await;
    assert!(matches!(denied, Err(Error::NotFound)));

    let mut elsewhere = album.request(provider::EDITION_ID);
    elsewhere.recording_id = Uuid::new_v4().to_string();
    let unrelated = album
        .hito
        .catalog
        .album_edition_track_list_until(
            &album.actor,
            &album.release_id,
            elsewhere,
            std::future::pending(),
        )
        .await;
    assert!(matches!(unrelated, Err(Error::NotFound)));

    let mut mismatched = album.request(provider::EDITION_ID);
    mismatched.recording_id = album.second.clone();
    let wrong_association = album
        .hito
        .catalog
        .album_edition_track_list_until(
            &album.actor,
            &album.release_id,
            mismatched,
            std::future::pending(),
        )
        .await;
    assert!(matches!(wrong_association, Err(Error::NotFound)));
    assert_eq!(
        server.requests().len(),
        requests_before,
        "Denied reads fail before provider I/O"
    );

    let detail = album
        .hito
        .catalog
        .release(&album.actor, &album.release_id, Browse::default())
        .await
        .unwrap();
    let anchor = detail
        .catalog_anchor
        .expect("A reviewed track anchors the album");
    assert_eq!(anchor.recording_id, album.first);
    assert_eq!(anchor.association_id, album.association.id);
    assert!(server.finish().await);
}
