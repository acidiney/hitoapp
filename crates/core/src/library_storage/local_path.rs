use crate::{Error, Result};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub(super) fn encode(path: &Path) -> Result<String> {
    match path.to_str() {
        Some(path) => serde_json::to_string(path),
        None => serde_json::to_string(path.as_os_str()),
    }
    .map_err(|_| invalid())
}

pub(crate) fn decode(value: &str) -> Result<PathBuf> {
    let path = if let Ok(value) = serde_json::from_str::<String>(value) {
        PathBuf::from(value)
    } else {
        PathBuf::from(serde_json::from_str::<OsString>(value).map_err(|_| invalid())?)
    };
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(path)
}

fn invalid() -> Error {
    Error::Invalid("Local file locator is invalid".into())
}
