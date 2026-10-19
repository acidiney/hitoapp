use crate::{Error, MagnetFileBinding, binding, magnet};
use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};

pub struct MagnetFileSelection {
    pub info_hash: String,
    pub magnet_bytes: u64,
    pub file_path: Vec<String>,
    pub file_bytes: u64,
}

impl MagnetFileSelection {
    pub fn validate(&self) -> Result<(), Error> {
        binding::validate_selection(
            &self.info_hash,
            self.magnet_bytes,
            &self.file_path,
            self.file_bytes,
        )
    }
}

/// Upload acknowledgment metadata, not recording identity or playable audio.
/// Deserialization requires a trusted owned snapshot, not an external authenticity claim.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MagnetUploadReceipt {
    #[serde(deserialize_with = "restored_binding")]
    binding: MagnetFileBinding,
}

impl MagnetUploadReceipt {
    pub fn binding(&self) -> &MagnetFileBinding {
        &self.binding
    }
}

fn restored_binding<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<MagnetFileBinding, D::Error> {
    let binding = MagnetFileBinding::deserialize(deserializer)?;
    binding
        .validate()
        .map_err(|_| serde::de::Error::custom("The stored AllDebrid binding is invalid"))?;
    Ok(binding)
}

#[derive(Deserialize)]
struct UploadData {
    magnets: Vec<UploadRow>,
}

#[derive(Deserialize)]
struct UploadRow {
    magnet: String,
    hash: String,
    id: u64,
    size: u64,
    #[serde(rename = "ready")]
    _ready: bool,
    error: Option<IgnoredAny>,
}

pub(crate) fn receipt(
    bytes: &[u8],
    selection: &MagnetFileSelection,
) -> Result<MagnetUploadReceipt, Error> {
    let data: UploadData = magnet::parse(bytes)?;
    if data.magnets.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let row = &data.magnets[0];
    if row.id == 0 || row.error.is_some() {
        return Err(Error::InvalidResponse);
    }
    if row.magnet != selection.info_hash
        || row.hash != selection.info_hash
        || row.size != selection.magnet_bytes
    {
        return Err(Error::SourceChanged);
    }
    Ok(MagnetUploadReceipt {
        binding: MagnetFileBinding {
            magnet_id: row.id,
            info_hash: row.hash.clone(),
            magnet_bytes: row.size,
            file_path: selection.file_path.clone(),
            file_bytes: selection.file_bytes,
        },
    })
}
