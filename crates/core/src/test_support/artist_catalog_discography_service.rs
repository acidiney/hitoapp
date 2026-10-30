pub use super::observed::ObservedRequest;
use super::observed::observed;
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CATALOG_VERSION,
    Capability, CatalogArtistKind, CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities,
    CatalogCreditMember, CatalogOperation, CatalogReleaseGroup, CatalogReleaseGroupKind, Manifest,
    PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const CAPABILITIES: &str = "v1/catalog/capabilities";
const PROFILE: &str = "v1/catalog/artists/profile";
const DISCOGRAPHY: &str = "v1/catalog/artists/discography";
const REQUEST_LIMIT: usize = 16;

pub const ARTIST_ID: &str = "opaque:artista/á?credit=lead&edição=一";
pub const PROVIDER_CURSOR: &str = "opaque:discografia/次?cursor=2%2F&assinatura=privada";

pub struct CatalogServer {
    pub url: String,
    plugin: FakePlugin,
    discography_enabled: Arc<AtomicBool>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let discography_enabled = Arc::new(AtomicBool::new(false));
        let plugin = FakePlugin::builder(Manifest {
            id: "org.hito.artist-discography-fixture".into(),
            name: "Owned artist discography".into(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: [Capability::Catalog].into(),
            authorization: None,
            content_policy: None,
        })
        .get(CAPABILITIES, {
            let enabled = discography_enabled.clone();
            move |_| {
                let mut operations = vec![CatalogOperation::ArtistProfile];
                if enabled.load(Ordering::SeqCst) {
                    operations.push(CatalogOperation::ArtistDiscography);
                }
                fake::Reply::Json(
                    serde_json::to_value(CatalogCapabilities {
                        catalog_version: CATALOG_VERSION,
                        operations,
                    })
                    .unwrap(),
                )
            }
        })
        .post(PROFILE, |request| {
            match serde_json::from_value::<ArtistProfileRequest>(request.json()) {
                Ok(parsed) if parsed.validate().is_ok() && parsed.artist_id == ARTIST_ID => {
                    fake::Reply::Json(serde_json::to_value(expected_profile()).unwrap())
                }
                _ => fake::Reply::Refuse,
            }
        })
        .post(DISCOGRAPHY, {
            let enabled = discography_enabled.clone();
            move |request| {
                let Ok(parsed) = serde_json::from_value::<ArtistDiscographyRequest>(request.json())
                else {
                    return fake::Reply::Refuse;
                };
                if !enabled.load(Ordering::SeqCst)
                    || parsed.validate().is_err()
                    || parsed.artist_id != ARTIST_ID
                    || parsed.snapshot.is_some()
                    || parsed.limit != 1
                    || !matches!(parsed.cursor.as_deref(), None | Some(PROVIDER_CURSOR))
                {
                    return fake::Reply::Refuse;
                }
                fake::Reply::Json(
                    serde_json::to_value(expected_page(parsed.cursor.is_some())).unwrap(),
                )
            }
        })
        .strict(REQUEST_LIMIT)
        .start()
        .await;
        Self {
            url: plugin.url.clone(),
            plugin,
            discography_enabled,
        }
    }

    pub fn enable_discography(&self) {
        self.discography_enabled.store(true, Ordering::SeqCst);
    }

    pub fn requests(&self) -> Vec<ObservedRequest> {
        observed(&self.plugin, REQUEST_LIMIT)
    }

    pub fn unexpected(&self) -> usize {
        self.plugin.unexpected()
    }

    pub async fn finish(&mut self) -> bool {
        self.plugin.finish().await
    }
}

pub fn expected_profile() -> CatalogArtistProfile {
    CatalogArtistProfile {
        artist: CatalogArtistRef {
            id: ARTIST_ID.into(),
            name: "Conjunto da sessão".into(),
        },
        sort_name: None,
        kind: Some(CatalogArtistKind::Group),
        disambiguation: None,
        revision: None,
    }
}

pub fn expected_page(last: bool) -> ArtistDiscographyPage {
    let group = if last {
        CatalogReleaseGroup {
            id: "opaque:release-group/estúdio?edição=二".into(),
            title: "Sessão nº 1".into(),
            artist_credit: None,
            kind: None,
            secondary_types: vec![],
            first_release_date: None,
            revision: None,
        }
    } else {
        CatalogReleaseGroup {
            id: "opaque:release-group/ao-vivo?edição=一".into(),
            title: "Sessão nº 1".into(),
            artist_credit: Some(vec![
                CatalogCreditMember {
                    artist: Some(expected_profile().artist),
                    credited_name: "O conjunto da sessão".into(),
                    join_phrase: " & ".into(),
                },
                CatalogCreditMember {
                    artist: None,
                    credited_name: "Voz convidada".into(),
                    join_phrase: String::new(),
                },
            ]),
            kind: Some(CatalogReleaseGroupKind::Album),
            secondary_types: vec!["Live".into()],
            first_release_date: Some("2024-02".into()),
            revision: None,
        }
    };
    ArtistDiscographyPage {
        artist_id: ARTIST_ID.into(),
        snapshot: None,
        items: vec![group],
        total_count: None,
        next_cursor: (!last).then(|| PROVIDER_CURSOR.into()),
    }
}
