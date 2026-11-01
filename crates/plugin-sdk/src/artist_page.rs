#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtistPageProvider {
    Spotify,
}

struct Shape {
    provider: ArtistPageProvider,
    prefix: &'static str,
    identifier_length: usize,
}

const SHAPES: [Shape; 1] = [Shape {
    provider: ArtistPageProvider::Spotify,
    prefix: "https://open.spotify.com/artist/",
    identifier_length: 22,
}];

pub fn artist_page_provider(url: &str) -> Option<ArtistPageProvider> {
    SHAPES.iter().find_map(|shape| {
        let identifier = url.strip_prefix(shape.prefix)?;
        (identifier.len() == shape.identifier_length
            && identifier.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then_some(shape.provider)
    })
}
