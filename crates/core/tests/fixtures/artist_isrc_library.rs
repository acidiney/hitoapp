use hito_core::test_support::{
    musicbrainz::{FakeMusicBrainz, Reply},
    playlist_service as provider, support,
};
use hito_core::{
    Actor, ArtistProviderMatchReceipt, ImportProviderPlaylist, MatchArtistsFromProviderEvidence,
    MatchArtistsFromTags, RecordingArtistIdentityState, Services,
};
use lofty::{
    prelude::{Accessor, ItemKey, TagExt},
    tag::{Tag, TagType},
};
use serde_json::{Value, json};
use sqlx::Connection;
use std::collections::HashMap;
use uuid::Uuid;

pub const CREDIT: &str = "C4 Pedro";
pub const ARTIST: &str = "11111111-1111-4111-8111-111111111111";
pub const OTHER_ARTIST: &str = "22222222-2222-4222-8222-222222222222";
pub const FEATURED: &str = "33333333-3333-4333-8333-333333333333";
pub const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";
pub const IMPORTED_ISRC: &str = "us-rc1-76-07839";
pub const IMPORTED_CODE: &str = "USRC17607839";
pub const AMBIGUOUS_CODE: &str = "GBAAA2000002";
pub const TWO_ARTISTS_CODE: &str = "GBAAA2000003";
pub const UNKNOWN_CODE: &str = "GBAAA2000004";
pub const COMPILATION_CODE: &str = "GBAAA2000005";
pub const OUTAGE_CODE: &str = "GBAAA2000006";
pub const LOCAL_CODE: &str = "GBAAA2000007";
pub const PAGE: &str = "https://open.spotify.com/artist/0000000000000000000001";
pub const SECOND_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000002";
pub const SHARED_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000003";
pub const UNLINKED_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000004";
pub const OUTAGE_PAGE: &str = "https://open.spotify.com/artist/0000000000000000000005";
const RECORDING_A: &str = "0f700000-0000-4000-8000-0000000000a1";
const RECORDING_B: &str = "0f700000-0000-4000-8000-0000000000a2";

pub struct Library {
    pub hito: Services,
    pub actor: Actor,
    pub musicbrainz: FakeMusicBrainz,
    pub origin: provider::ProviderServer,
    pub tagged: String,
    pub imported: String,
    pub ambiguous: String,
    pub two_artists: String,
    pub unknown: String,
    pub compilation: String,
    pub outage: String,
    pub without_isrc: String,
    pub page: String,
    pub page_with_isrc: String,
    pub two_pages: String,
    pub shared_page: String,
    pub unlinked_page: String,
    pub page_outage: String,
    pub local_isrc: String,
    pub installation: String,
    pub library: String,
    directory: tempfile::TempDir,
}

fn imported(id: &str, title: &str, isrc: Option<&str>) -> Value {
    imported_with_pages(id, title, isrc, &[])
}

fn imported_with_pages(id: &str, title: &str, isrc: Option<&str>, pages: &[&str]) -> Value {
    json!({"occurrence_id": id, "item": {"kind": "recording", "recording": {
        "id": id, "title": title, "artist": CREDIT, "duration_ms": 1000, "release": null,
        "disc_number": null, "track_number": null, "revision": "v1", "isrc": isrc,
        "artist_urls": pages
    }}})
}

impl Library {
    pub async fn prepare() -> Self {
        let musicbrainz = FakeMusicBrainz::start().await;
        musicbrainz.isrc(
            IMPORTED_CODE,
            Reply::Recordings(vec![(RECORDING_A.into(), vec![ARTIST.into()])]),
        );
        musicbrainz.isrc(
            AMBIGUOUS_CODE,
            Reply::Recordings(vec![
                (RECORDING_A.into(), vec![ARTIST.into()]),
                (RECORDING_B.into(), vec![OTHER_ARTIST.into()]),
            ]),
        );
        musicbrainz.isrc(
            TWO_ARTISTS_CODE,
            Reply::Recordings(vec![(
                RECORDING_A.into(),
                vec![ARTIST.into(), FEATURED.into()],
            )]),
        );
        musicbrainz.isrc(
            COMPILATION_CODE,
            Reply::Recordings(vec![(RECORDING_A.into(), vec![VARIOUS_ARTISTS.into()])]),
        );
        musicbrainz.isrc(OUTAGE_CODE, Reply::Unavailable);
        musicbrainz.url(PAGE, Reply::Artists(vec![ARTIST.into()]));
        musicbrainz.url(SECOND_PAGE, Reply::Artists(vec![OTHER_ARTIST.into()]));
        musicbrainz.url(
            SHARED_PAGE,
            Reply::Artists(vec![ARTIST.into(), FEATURED.into()]),
        );
        musicbrainz.url(UNLINKED_PAGE, Reply::NotFound);
        musicbrainz.url(OUTAGE_PAGE, Reply::Unavailable);
        musicbrainz.isrc(
            LOCAL_CODE,
            Reply::Recordings(vec![(RECORDING_B.into(), vec![OTHER_ARTIST.into()])]),
        );
        let (directory, hito, actor, library, music) =
            support::library_with_musicbrainz("artist-isrc", Some(musicbrainz.endpoint.clone()))
                .await;
        for (name, offset, title, artist, mbid, isrc) in [
            ("a.wav", 1, "Song A", CREDIT, Some(ARTIST), None),
            (
                "l.wav",
                2,
                "Local ISRC",
                "Someone Else",
                None,
                Some(LOCAL_CODE),
            ),
        ] {
            let path = music.join(name);
            support::write_wav(&path, offset);
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title(title.into());
            tag.set_artist(artist.into());
            if let Some(mbid) = mbid {
                tag.insert_text(ItemKey::MusicBrainzArtistId, mbid.into());
            }
            if let Some(isrc) = isrc {
                tag.insert_text(ItemKey::Isrc, isrc.into());
            }
            tag.save_to_path(&path, Default::default()).unwrap();
        }
        support::scan(&hito, &actor, &library).await;
        let origin = provider::ProviderServer::start().await;
        let installation = provider::connect(&hito, &actor, &origin, &["playlists"]).await;
        provider::playlist(
            &origin,
            "owned-isrc",
            "owned-isrc-v1",
            vec![
                imported("imported", "Imported Song", Some(IMPORTED_ISRC)),
                imported("ambiguous", "Ambiguous Song", Some(AMBIGUOUS_CODE)),
                imported("two", "Two Artists Song", Some(TWO_ARTISTS_CODE)),
                imported("unknown", "Unknown Song", Some(UNKNOWN_CODE)),
                imported("compilation", "Compilation Song", Some(COMPILATION_CODE)),
                imported("outage", "Outage Song", Some(OUTAGE_CODE)),
                imported("plain", "Plain Song", None),
                imported_with_pages("page", "Page Song", None, &[PAGE]),
                imported_with_pages("both", "Both Song", Some(UNKNOWN_CODE), &[SECOND_PAGE]),
                imported_with_pages("two-pages", "Two Pages Song", None, &[PAGE, SECOND_PAGE]),
                imported_with_pages("shared", "Shared Page Song", None, &[SHARED_PAGE]),
                imported_with_pages("unlinked", "Unlinked Page Song", None, &[UNLINKED_PAGE]),
                imported_with_pages("page-outage", "Page Outage Song", None, &[OUTAGE_PAGE]),
            ],
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
                    playlist_reference: "https://provider.invalid/owned-isrc".into(),
                },
            )
            .await
            .unwrap();
        let playlist = hito
            .collection
            .playlist(&actor, &receipt.playlist_id.to_string())
            .await
            .unwrap();
        let ids: Vec<String> = playlist
            .entries
            .iter()
            .map(|entry| entry.recording_id.clone().unwrap())
            .collect();
        let mut local = HashMap::new();
        for item in hito
            .catalog
            .recordings(&actor, Default::default())
            .await
            .unwrap()
            .items
        {
            local.insert(item.title.clone(), item.id.clone());
        }
        Self {
            tagged: local["Song A"].clone(),
            local_isrc: local["Local ISRC"].clone(),
            imported: ids[0].clone(),
            ambiguous: ids[1].clone(),
            two_artists: ids[2].clone(),
            unknown: ids[3].clone(),
            compilation: ids[4].clone(),
            outage: ids[5].clone(),
            without_isrc: ids[6].clone(),
            page: ids[7].clone(),
            page_with_isrc: ids[8].clone(),
            two_pages: ids[9].clone(),
            shared_page: ids[10].clone(),
            unlinked_page: ids[11].clone(),
            page_outage: ids[12].clone(),
            installation: installation.id,
            library,
            hito,
            actor,
            musicbrainz,
            origin,
            directory,
        }
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

    pub async fn run(&self) -> ArtistProviderMatchReceipt {
        self.run_as(&self.actor).await
    }

    pub async fn run_as(&self, actor: &Actor) -> ArtistProviderMatchReceipt {
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
            let receipt = self
                .hito
                .artist_catalog
                .match_artists_from_provider_evidence(
                    actor,
                    MatchArtistsFromProviderEvidence {
                        request_id: Uuid::new_v4(),
                        after: after.clone(),
                        limit: None,
                    },
                )
                .await
                .unwrap();
            total.matched += receipt.matched;
            total.uncertain += receipt.uncertain;
            total.declined += receipt.declined;
            total.unavailable += receipt.unavailable;
            match receipt.next_after {
                Some(next) => after = Some(next),
                None => return total,
            }
        }
        panic!("The provider evidence walk must end");
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

    pub async fn plant_page(&self, recording: &str, url: &str) {
        let mut connection = sqlx::SqliteConnection::connect(&format!(
            "sqlite://{}?mode=rw",
            self.directory.path().join("hito.db").display()
        ))
        .await
        .unwrap();
        sqlx::query("INSERT INTO recording_artist_url_observations(observation_id,recording_id,url) SELECT metadata_observation_id,id,? FROM recordings WHERE id=?")
            .bind(url)
            .bind(recording)
            .execute(&mut connection)
            .await
            .unwrap();
    }
}
