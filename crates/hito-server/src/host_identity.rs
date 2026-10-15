use crate::config::RuntimeResult;
use hito_core::HostDeviceId;
use std::path::Path;

pub async fn load(
    database: &Path,
    override_device: Option<HostDeviceId>,
) -> RuntimeResult<HostDeviceId> {
    if let Some(device) = override_device {
        return Ok(device);
    }
    let identity = database.with_extension("device-id");
    if !tokio::fs::try_exists(&identity).await? {
        let legacy_database = tokio::fs::try_exists(database).await?;
        let value = if legacy_database {
            "server".to_string()
        } else {
            uuid::Uuid::new_v4().to_string()
        };
        let temporary = database.with_extension(format!("{}.device-id", uuid::Uuid::new_v4()));
        tokio::fs::write(&temporary, &value).await?;
        let linked = tokio::fs::hard_link(&temporary, &identity).await;
        tokio::fs::remove_file(temporary).await?;
        if let Err(error) = linked
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error.into());
        }
        tracing::info!(
            event = "host_identity_initialized",
            legacy_database,
            "Backend host identity persisted"
        );
    }
    Ok(HostDeviceId::new(
        tokio::fs::read_to_string(identity).await?.trim(),
    )?)
}
