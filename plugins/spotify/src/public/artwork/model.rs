use super::MAX_DIMENSION;
use crate::{error::Error, public::session};
use hito_plugin_sdk::{ArtworkEntity, ArtworkRequest};
use serde::Deserialize;

#[derive(Deserialize)]
struct Bootstrap {
    props: Props,
}

#[derive(Deserialize)]
struct Props {
    #[serde(rename = "pageProps")]
    page: Page,
}

#[derive(Deserialize)]
struct Page {
    state: State,
}

#[derive(Deserialize)]
struct State {
    data: Data,
}

#[derive(Deserialize)]
struct Data {
    entity: Entity,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entity {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    uri: String,
    visual_identity: Option<VisualIdentity>,
    cover_art: Option<CoverArt>,
}

#[derive(Deserialize)]
struct VisualIdentity {
    image: Vec<Image>,
}

#[derive(Deserialize)]
struct CoverArt {
    sources: Vec<Image>,
}

#[derive(Deserialize)]
struct Image {
    url: String,
    #[serde(rename = "maxWidth", alias = "width")]
    width: u32,
    #[serde(rename = "maxHeight", alias = "height")]
    height: u32,
}

pub(super) fn location(bytes: &[u8], request: &ArtworkRequest) -> Result<String, Error> {
    let html = std::str::from_utf8(bytes).map_err(|_| Error::InvalidResponse)?;
    let script = session::script_contents(html)?;
    let bootstrap: Bootstrap = serde_json::from_str(script).map_err(|_| Error::InvalidResponse)?;
    let entity = bootstrap.props.page.state.data.entity;
    let kind = match request.entity {
        ArtworkEntity::Artist => "artist",
        ArtworkEntity::Playlist => "playlist",
        ArtworkEntity::Recording => "track",
        ArtworkEntity::Release => "album",
        ArtworkEntity::Featured => return Err(Error::InvalidArtworkRequest),
    };
    if entity.kind != kind
        || entity.id != request.provider_id
        || entity.uri != format!("spotify:{kind}:{}", request.provider_id)
    {
        return Err(Error::InvalidResponse);
    }
    let cover = entity
        .cover_art
        .map(|cover| cover.sources)
        .unwrap_or_default();
    let visual = entity
        .visual_identity
        .map(|visual| visual.image)
        .unwrap_or_default();
    if cover.len() + visual.len() > 32 {
        return Err(Error::InvalidResponse);
    }
    let images = if cover.is_empty() { visual } else { cover };
    if images.is_empty() {
        return Err(Error::ArtworkNotFound);
    }
    images
        .into_iter()
        .filter(|image| {
            image.width > 0
                && image.height > 0
                && image.width <= MAX_DIMENSION
                && image.height <= MAX_DIMENSION
        })
        .max_by_key(|image| u64::from(image.width) * u64::from(image.height))
        .map(|image| image.url)
        .ok_or(Error::InvalidResponse)
}
