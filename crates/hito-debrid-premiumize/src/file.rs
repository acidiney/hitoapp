use crate::{Error, TransferFileBinding, TransferPhase, observation};
use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Transfers {
    transfers: Vec<FileReference>,
}

#[derive(Deserialize)]
struct FileReference {
    id: String,
    file_id: Option<String>,
    folder_id: Option<String>,
}

pub(crate) fn validate_transfer(bytes: &[u8], binding: &TransferFileBinding) -> Result<(), Error> {
    let (phase, _) = observation::transfer(bytes, &binding.transfer)?;
    if !matches!(
        phase,
        TransferPhase::ProviderCompleted | TransferPhase::ProviderSeeding
    ) {
        return Err(Error::UnsupportedState);
    }
    let references: Transfers =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    let reference = references
        .transfers
        .iter()
        .find(|reference| reference.id == binding.transfer.transfer_id)
        .ok_or(Error::TransferNotFound)?;
    let (Some(item_id), Some(folder_id)) = (&reference.file_id, &reference.folder_id) else {
        return Err(Error::UnsupportedState);
    };
    if item_id.is_empty() || folder_id.is_empty() {
        return Err(Error::UnsupportedState);
    }
    if item_id != &binding.item_id || folder_id != &binding.folder_id {
        return Err(Error::FileChanged);
    }
    Ok(())
}

#[derive(Deserialize)]
struct Item {
    status: String,
    code: Option<String>,
    id: Option<String>,
    name: Option<String>,
    size: Option<u64>,
    created_at: Option<u64>,
    folder_id: Option<String>,
    #[serde(default, deserialize_with = "private_link")]
    link: Option<Zeroizing<String>>,
}

pub(crate) fn bound_link(
    bytes: &[u8],
    binding: &TransferFileBinding,
) -> Result<Zeroizing<String>, Error> {
    let item: Item = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    observation::provider_result(&item.status, item.code.as_deref())?;
    let id = item.id.as_deref().ok_or(Error::InvalidResponse)?;
    let name = item.name.as_deref().ok_or(Error::InvalidResponse)?;
    let size = item.size.ok_or(Error::InvalidResponse)?;
    let created_at = item.created_at.ok_or(Error::InvalidResponse)?;
    let folder_id = item.folder_id.as_deref().ok_or(Error::InvalidResponse)?;
    if id != binding.item_id
        || name != binding.file_name
        || size != binding.file_bytes
        || created_at != binding.item_created_at_unix_seconds
        || folder_id != binding.folder_id
    {
        return Err(Error::FileChanged);
    }
    item.link.ok_or(Error::InvalidResponse)
}

fn private_link<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Zeroizing<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|value| value.map(Zeroizing::new))
}
