use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Configuration,
    InvalidRequest,
    InvalidDiscographyRequest,
    InvalidSearchRequest,
    Transport,
    Status(u16),
    InvalidResponse,
    InvalidDiscography,
    InvalidSearch,
    ResponseTooLarge,
}

impl Error {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Configuration => "configuration_error",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidDiscographyRequest => "invalid_discography_request",
            Self::InvalidSearchRequest => "invalid_search_request",
            Self::Transport => "transport_error",
            Self::Status(_) => "status_error",
            Self::InvalidResponse => "invalid_response",
            Self::InvalidDiscography => "invalid_discography",
            Self::InvalidSearch => "invalid_search",
            Self::ResponseTooLarge => "response_too_large",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Configuration => "The MusicBrainz client could not be initialized",
            Self::InvalidRequest => "Use a canonical MusicBrainz artist identifier",
            Self::InvalidDiscographyRequest => "Use a valid MusicBrainz artist discography request",
            Self::InvalidSearchRequest => "Use a valid MusicBrainz artist search request",
            Self::Transport => "The MusicBrainz service could not be reached",
            Self::Status(_) => "The MusicBrainz service rejected the request",
            Self::InvalidResponse => "The MusicBrainz service returned an invalid artist profile",
            Self::InvalidDiscography => {
                "The MusicBrainz service returned an invalid artist discography"
            }
            Self::InvalidSearch => "The MusicBrainz service returned invalid artist search results",
            Self::ResponseTooLarge => "The MusicBrainz response exceeds the size limit",
        })
    }
}

impl std::error::Error for Error {}
