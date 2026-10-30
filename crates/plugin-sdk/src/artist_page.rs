/// Providers whose canonical artist pages Hito accepts as matching evidence.
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

/// Names the provider of an artist page address of exactly an allowlisted shape: the canonical
/// prefix followed by a base62 identifier of the provider's length and nothing else.
pub fn artist_page_provider(url: &str) -> Option<ArtistPageProvider> {
    SHAPES.iter().find_map(|shape| {
        let identifier = url.strip_prefix(shape.prefix)?;
        (identifier.len() == shape.identifier_length
            && identifier.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then_some(shape.provider)
    })
}
