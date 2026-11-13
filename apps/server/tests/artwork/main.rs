#![allow(clippy::duplicate_mod)]

mod artist_photo;
mod artwork_admission;
mod artwork_availability;
mod artwork_runtime;
mod collection_artwork;
mod credited_group_photo;
mod playlist_artwork;
mod provider_album_cover;

mod artwork_normalization;
#[path = "../fixtures/artwork_normalization.rs"]
mod artwork_normalization_fixture;
#[path = "../fixtures/artwork_normalization_provider.rs"]
mod artwork_normalization_provider;

#[path = "../fixtures/artwork_rendition_http.rs"]
mod artwork_rendition_http;
#[path = "../fixtures/artwork_rendition_images.rs"]
mod artwork_rendition_images;
mod artwork_renditions;

mod artwork_storage;

mod credited_artist_authority;

mod artwork_thousand;
#[path = "../fixtures/artwork_thousand_corpus.rs"]
mod artwork_thousand_corpus;
#[path = "../fixtures/artwork_thousand_http.rs"]
mod artwork_thousand_http;
#[path = "../fixtures/artwork_thousand_provider.rs"]
mod artwork_thousand_provider;
