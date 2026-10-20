use crate::{Error, ProviderRejection, TransferBinding, TransferPhase, TransferProgress};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    status: String,
    code: Option<String>,
    transfers: Option<Vec<Transfer>>,
}

#[derive(Deserialize)]
struct Transfer {
    id: String,
    created_at: u64,
    status: String,
    progress: f64,
}

pub(crate) fn transfer(
    bytes: &[u8],
    binding: &TransferBinding,
) -> Result<(TransferPhase, TransferProgress), Error> {
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    provider_result(&envelope.status, envelope.code.as_deref())?;
    let transfers = envelope.transfers.ok_or(Error::InvalidResponse)?;
    let mut matches = transfers
        .iter()
        .filter(|transfer| transfer.id == binding.transfer_id);
    let transfer = matches.next().ok_or(Error::TransferNotFound)?;
    if matches.next().is_some() || transfer.created_at != binding.created_at_unix_seconds {
        return Err(Error::TransferChanged);
    }
    let progress = TransferProgress::new(transfer.progress)?;
    let phase = match transfer.status.as_str() {
        "queued" if transfer.progress == 0.0 => TransferPhase::Queued,
        "running" => TransferPhase::Running,
        "error" if transfer.progress == 0.0 => TransferPhase::ProviderFailed,
        "seeding" => TransferPhase::ProviderSeeding,
        "finished" if transfer.progress == 1.0 => TransferPhase::ProviderCompleted,
        "queued" | "error" | "finished" => return Err(Error::InvalidResponse),
        _ => return Err(Error::UnsupportedState),
    };
    Ok((phase, progress))
}

pub(crate) fn provider_result(status: &str, code: Option<&str>) -> Result<(), Error> {
    if status == "error" {
        let reason = match code {
            Some("authentication_failed") => ProviderRejection::AuthenticationFailed,
            Some("permission_denied") => ProviderRejection::PermissionDenied,
            Some("rate_limit_reached") => ProviderRejection::RateLimited,
            Some("not_found") => ProviderRejection::NotFound,
            _ => ProviderRejection::Rejected,
        };
        return Err(Error::ProviderRejected(reason));
    }
    if status != "success" || code.is_some() {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}
