pub use super::observed::ObservedRequest;
use super::observed::observed;
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CATALOG_VERSION,
    Capability, CatalogArtistKind, CatalogArtistProfile, CatalogArtistRef, CatalogCapabilities,
    CatalogCreditMember, CatalogDiscographyCapabilities, CatalogDiscographyOperation,
    CatalogOperation, CatalogReleaseGroup, CatalogReleaseGroupKind, CatalogReleaseTypeFilter,
    Manifest, PROTOCOL_VERSION,
};
use hito_testkit::fake_plugin::{self as fake, FakePlugin};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const CAPABILITIES: &str = "v1/catalog/capabilities";
const PROFILE: &str = "v1/catalog/artists/profile";
const DISCOGRAPHY: &str = "v1/catalog/artists/discography";
const FILTER_CAPABILITIES: &str = "v1/catalog/discography/capabilities";
const REQUEST_LIMIT: usize = 16;

pub const ARTIST_ID: &str = "opaque:artista/á?credit=lead&edição=一";
pub const PROVIDER_CURSOR: &str = "opaque:discografia/次?cursor=2%2F&assinatura=privada";
pub const TYPED_CURSOR: &str = "opaque:tipo/álbum?cursor=2";

pub struct CatalogServer {
    pub url: String,
    plugin: FakePlugin,
    discography_enabled: Arc<AtomicBool>,
    filter_enabled: Arc<AtomicBool>,
    typed_outage: Arc<AtomicBool>,
}

impl CatalogServer {
    pub async fn start() -> Self {
        let discography_enabled = Arc::new(AtomicBool::new(false));
        let filter_enabled = Arc::new(AtomicBool::new(false));
        let typed_outage = Arc::new(AtomicBool::new(false));
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
        .get(FILTER_CAPABILITIES, {
            let enabled = filter_enabled.clone();
            move |_| {
                if !enabled.load(Ordering::SeqCst) {
                    return fake::Reply::Status(axum::http::StatusCode::NOT_FOUND);
                }
                fake::Reply::Json(
                    serde_json::to_value(CatalogDiscographyCapabilities {
                        discography_version: 1,
                        operations: vec![CatalogDiscographyOperation::ReleaseTypeFilter],
                    })
                    .unwrap(),
                )
            }
        })
        .post(DISCOGRAPHY, {
            let enabled = discography_enabled.clone();
            let filter_enabled = filter_enabled.clone();
            let typed_outage = typed_outage.clone();
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
                    || !matches!(
                        parsed.cursor.as_deref(),
                        None | Some(PROVIDER_CURSOR | TYPED_CURSOR)
                    )
                {
                    return fake::Reply::Refuse;
                }
                if let Some(filter) = parsed.release_type {
                    if !filter_enabled.load(Ordering::SeqCst) {
                        return fake::Reply::Refuse;
                    }
                    if typed_outage.load(Ordering::SeqCst) {
                        return fake::Reply::Status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
                    }
                    return match typed_page(filter, parsed.cursor.as_deref()) {
                        Some(page) => fake::Reply::Json(serde_json::to_value(page).unwrap()),
                        None => fake::Reply::Refuse,
                    };
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
            filter_enabled,
            typed_outage,
        }
    }

    pub fn enable_type_filter(&self) {
        self.filter_enabled.store(true, Ordering::SeqCst);
    }

    pub fn fail_typed_reads(&self, failing: bool) {
        self.typed_outage.store(failing, Ordering::SeqCst);
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

pub fn typed_group(id: &str, kind: CatalogReleaseGroupKind) -> CatalogReleaseGroup {
    CatalogReleaseGroup {
        id: id.into(),
        title: "Sessão tipada".into(),
        artist_credit: None,
        kind: Some(kind),
        secondary_types: vec![],
        first_release_date: None,
        revision: None,
    }
}

pub fn typed_page(
    filter: CatalogReleaseTypeFilter,
    cursor: Option<&str>,
) -> Option<ArtistDiscographyPage> {
    let (items, next_cursor) = match (filter, cursor) {
        (CatalogReleaseTypeFilter::Album, None) => (
            vec![typed_group(
                "opaque:release-group/album-1",
                CatalogReleaseGroupKind::Album,
            )],
            Some(TYPED_CURSOR.to_owned()),
        ),
        (CatalogReleaseTypeFilter::Album, Some(TYPED_CURSOR)) => (
            vec![typed_group(
                "opaque:release-group/album-2",
                CatalogReleaseGroupKind::Album,
            )],
            None,
        ),
        (CatalogReleaseTypeFilter::Ep, None) => (
            vec![typed_group(
                "opaque:release-group/ep-1",
                CatalogReleaseGroupKind::Ep,
            )],
            None,
        ),
        (CatalogReleaseTypeFilter::Single | CatalogReleaseTypeFilter::Other, None) => {
            (vec![], None)
        }
        _ => return None,
    };
    Some(ArtistDiscographyPage {
        artist_id: ARTIST_ID.into(),
        snapshot: None,
        items,
        total_count: None,
        next_cursor,
    })
}
