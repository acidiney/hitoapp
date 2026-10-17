use crate::{
    Error, Result,
    plugins::{
        Plugins,
        transport::{Admission, Call, ControlClient},
    },
};
use hito_plugin_sdk::{
    PREPARATION_STATUS_PATH, PREPARE_SOURCE_PATH, PreparationRequest, PreparationStatus,
    PreparationStatusRequest, PreparationStatusResponse, PreparationSubmission,
    PreparationSubmitResponse,
};
use sqlx::Row;
use uuid::Uuid;
use zeroize::Zeroizing;

pub(crate) struct PreparationOperation {
    remote: ControlClient,
    base_url: String,
    token: Option<Zeroizing<String>>,
    request: PreparationRequest,
    handle: Option<String>,
    _admission: Admission,
}

pub(crate) enum PreparationExchange {
    Submission(PreparationSubmission),
    Status(PreparationStatus),
}

impl Plugins {
    pub(crate) async fn preparation_operation(
        &self,
        preparation: &str,
    ) -> Result<PreparationOperation> {
        let row = sqlx::query("SELECT preparation.user_id,preparation.installation_id,preparation.service_id,
            preparation.credential,preparation.provider_recording_id,preparation.remote_source_id,
            preparation.remote_handle,preparation.submission_state,service.base_url
            FROM source_preparations preparation JOIN current_source_preparation_authority authority ON authority.id = preparation.id
            JOIN plugin_services service ON service.id = preparation.service_id WHERE preparation.id = ?")
            .bind(preparation).fetch_optional(&self.store.pool).await?.ok_or(Error::NotFound)?;
        let account: String = row.try_get("user_id")?;
        let installation: String = row.try_get("installation_id")?;
        let service: String = row.try_get("service_id")?;
        let credential: Option<Vec<u8>> = row.try_get("credential")?;
        let token = credential
            .as_deref()
            .map(|credential| {
                self.vault
                    .reveal(&account, &installation, &service, credential)
            })
            .transpose()
            .map_err(|_| Error::Invalid("Plugin service authentication is unavailable".into()))?;
        let request = PreparationRequest {
            operation_id: Uuid::parse_str(preparation)
                .map_err(|_| Error::Internal("Stored preparation identity is invalid".into()))?,
            recording_id: row.try_get("provider_recording_id")?,
            source_id: row.try_get("remote_source_id")?,
        };
        request
            .validate()
            .map_err(|_| Error::Internal("Stored preparation target is invalid".into()))?;
        let handle: Option<String> = row.try_get("remote_handle")?;
        let state: String = row.try_get("submission_state")?;
        if !matches!(
            (state.as_str(), handle.as_ref()),
            ("unsubmitted", None) | ("submitted", Some(_))
        ) {
            return Err(Error::Internal(
                "Stored preparation submission state is invalid".into(),
            ));
        }
        if let Some(handle) = &handle {
            PreparationStatusRequest {
                operation_id: request.operation_id,
                recording_id: request.recording_id.clone(),
                source_id: request.source_id.clone(),
                handle: handle.clone(),
            }
            .validate()
            .map_err(|_| Error::Internal("Stored preparation handle is invalid".into()))?;
        }
        let admission = self.remote.admit_preparation(&account)?;
        Ok(PreparationOperation {
            remote: self.remote.clone(),
            base_url: row.try_get("base_url")?,
            token,
            request,
            handle,
            _admission: admission,
        })
    }
}

impl PreparationOperation {
    pub(crate) fn submits(&self) -> bool {
        self.handle.is_none()
    }

    pub(crate) async fn exchange(&self) -> Result<PreparationExchange> {
        let mut remaining = 16384;
        if let Some(handle) = &self.handle {
            let request = PreparationStatusRequest {
                operation_id: self.request.operation_id,
                recording_id: self.request.recording_id.clone(),
                source_id: self.request.source_id.clone(),
                handle: handle.clone(),
            };
            let body = serde_json::to_value(&request)
                .map_err(|_| Error::Internal("Preparation status could not be encoded".into()))?;
            let response: PreparationStatusResponse = self
                .remote
                .exchange_admitted(
                    &self.base_url,
                    self.token.as_deref().map(String::as_str),
                    Call {
                        path: PREPARATION_STATUS_PATH,
                        body: Some(body),
                        maximum: 16384,
                    },
                    &mut remaining,
                )
                .await?;
            response.validate(&request).map_err(|_| {
                Error::Upstream("The provider returned an invalid preparation status")
            })?;
            Ok(PreparationExchange::Status(response.status))
        } else {
            let body = serde_json::to_value(&self.request)
                .map_err(|_| Error::Internal("Source preparation could not be encoded".into()))?;
            let response: PreparationSubmitResponse = self
                .remote
                .exchange_admitted(
                    &self.base_url,
                    self.token.as_deref().map(String::as_str),
                    Call {
                        path: PREPARE_SOURCE_PATH,
                        body: Some(body),
                        maximum: 16384,
                    },
                    &mut remaining,
                )
                .await?;
            response.validate(&self.request).map_err(|_| {
                Error::Upstream("The provider returned an invalid preparation acknowledgement")
            })?;
            Ok(PreparationExchange::Submission(response.submission))
        }
    }
}
