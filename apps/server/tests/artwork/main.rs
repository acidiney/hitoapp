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
