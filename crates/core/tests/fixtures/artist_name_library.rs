use hito_core::test_support::{
    musicbrainz::FakeMusicBrainz, playlist_service as provider, support,
};
use hito_core::{
    Actor, ArtistProviderMatchReceipt, ImportProviderPlaylist, MatchArtistsFromProviderEvidence,
    MatchArtistsFromTags, RecordingArtistIdentityState, Services,
};
use serde_json::{Value, json};
use sqlx::Connection;
use uuid::Uuid;

pub const CREDIT: &str = "Nova Quartet";
pub const NOVA: &str = "44444444-4444-4444-8444-444444444444";
pub const NAMESAKE: &str = "55555555-5555-4555-8555-555555555555";
pub const TRIBUTE: &str = "66666666-6666-4666-8666-666666666666";

/// Title and length in milliseconds of the imported recordings of one credit.
pub const TRACKS: [(&str, u64); 4] = [
    ("Morning Light", 201_000),
    ("Second Wind", 187_500),
    ("Harbor Lights", 243_000),
    ("Quiet Rooms", 150_000),
];

pub struct NameLibrary {
    pub hito: Services,
    pub actor: Actor,
    pub musicbrainz: FakeMusicBrainz,
    pub origin: provider::ProviderServer,
    pub recordings: Vec<String>,
    directory: tempfile::TempDir,
}

fn imported(index: usize, credit: &str, title: &str, length: u64) -> Value {
    let id = format!("name-{index}");
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": id, "title": title, "artist": credit, "duration_ms": length, "release": null,
        "disc_number": null, "track_number": null, "revision": "v1", "isrc": null,
        "artist_urls": []
    }}})
}

/// Reply to every lookup of the default credit: no namesake is known to MusicBrainz.
pub fn nova_discography(
    items: &[(&str, Option<u64>)],
) -> hito_core::test_support::musicbrainz::Reply {
    hito_core::test_support::musicbrainz::Reply::Discography(
        items.len() as u32,
        items
            .iter()
            .map(|(title, length)| ((*title).to_owned(), *length))
            .collect(),
    )
}

impl NameLibrary {
    /// Imports one playlist whose recordings carry no identifier, ISRC or artist page, each
    /// listed as (credit, title, length in milliseconds).
    pub async fn prepare(entries: &[(&str, &str, u64)]) -> Self {
        let musicbrainz = FakeMusicBrainz::start().await;
        let (directory, hito, actor, _library, _music) =
            support::library_with_musicbrainz("artist-name", Some(musicbrainz.endpoint.clone()))
                .await;
        let origin = provider::ProviderServer::start().await;
        let installation = provider::connect(&hito, &actor, &origin, &["playlists"]).await;
        provider::playlist(
            &origin,
            "owned-name",
            "owned-name-v1",
            entries
                .iter()
                .enumerate()
                .map(|(index, (credit, title, length))| imported(index, credit, title, *length))
                .collect(),
        )
        .await;
        let receipt = hito
            .collection
            .import_provider_playlist(
                &actor,
                &installation.id,
                ImportProviderPlaylist {
                    request_id: Uuid::new_v4(),
                    installation_revision: installation.revision,
                    playlist_reference: "https://provider.invalid/owned-name".into(),
                },
            )
            .await
            .unwrap();
        let playlist = hito
            .collection
            .playlist(&actor, &receipt.playlist_id.to_string())
            .await
            .unwrap();
        let recordings = playlist
            .entries
            .iter()
            .map(|entry| entry.recording_id.clone().unwrap())
            .collect();
        Self {
            hito,
            actor,
            musicbrainz,
            origin,
            recordings,
            directory,
        }
    }

    /// The default library: the four `TRACKS` of one credit.
    pub async fn nova() -> Self {
        let entries: Vec<(&str, &str, u64)> = TRACKS
            .iter()
            .map(|(title, length)| (CREDIT, *title, *length))
            .collect();
        Self::prepare(&entries).await
    }

    pub async fn tag_pass(&self) {
        self.hito
            .artist_catalog
            .match_artists_from_tags(
                &self.actor,
                MatchArtistsFromTags {
                    request_id: Uuid::new_v4(),
                    after: None,
                    limit: None,
                },
            )
            .await
            .unwrap();
    }

    pub async fn request(
        &self,
        after: Option<String>,
    ) -> hito_core::Result<ArtistProviderMatchReceipt> {
        self.hito
            .artist_catalog
            .match_artists_from_provider_evidence(
                &self.actor,
                MatchArtistsFromProviderEvidence {
                    request_id: Uuid::new_v4(),
                    after,
                    limit: None,
                },
            )
            .await
    }

    /// Walks the cursor like the web app and sums the receipts.
    pub async fn run(&self) -> ArtistProviderMatchReceipt {
        let mut total = ArtistProviderMatchReceipt {
            request_id: Uuid::new_v4(),
            matched: 0,
            uncertain: 0,
            declined: 0,
            unavailable: 0,
            next_after: None,
        };
        let mut after = None;
        for _ in 0..32 {
            let receipt = self.request(after.clone()).await.unwrap();
            total.matched += receipt.matched;
            total.uncertain += receipt.uncertain;
            total.declined += receipt.declined;
            total.unavailable += receipt.unavailable;
            match receipt.next_after {
                Some(next) => after = Some(next),
                None => return total,
            }
        }
        panic!("The provider walk must end");
    }

    pub async fn group_of(&self, recording: &str) -> Option<String> {
        match self
            .hito
            .artist_catalog
            .recording_artist_identity_choice(&self.actor, recording)
            .await
            .unwrap()
            .state
        {
            RecordingArtistIdentityState::Selected(selection) => Some(selection.group_id),
            _ => None,
        }
    }

    pub async fn associations(&self, recording: &str) -> Vec<hito_core::ArtistAssociation> {
        self.hito
            .artist_catalog
            .recording_artist_associations(&self.actor, recording)
            .await
            .unwrap()
    }

    pub async fn rows(&self, table: &str) -> i64 {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=ro",
            self.directory.path().join("hito.db").display()
        ))
        .await
        .unwrap();
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut connection)
            .await
            .unwrap()
    }
}
