mod history;
mod projection;
mod read;
mod recording_fields;
mod release_fields;
mod release_read;
mod types;

pub(crate) use recording_fields::{
    APPEARANCE_ARTIST, APPEARANCE_TITLE, RECORDING_ARTIST, RECORDING_TITLE, TextField,
    join_recording_fields, set_recording_field,
};
pub use types::*;

pub(crate) use projection::release as refresh_release;
pub(crate) use projection::{provision, source as refresh_source};

pub(crate) use release_fields::set_release_field;

fn validate_text(value: Option<&str>) -> crate::Result<()> {
    if value.is_some_and(|value| {
        value.trim().is_empty() || value.len() > 2048 || value.chars().any(char::is_control)
    }) {
        return Err(crate::Error::Invalid(
            "Use metadata of 1 to 2048 bytes without control characters".into(),
        ));
    }
    Ok(())
}
