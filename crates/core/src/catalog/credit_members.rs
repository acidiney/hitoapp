use crate::Result;
use hito_plugin_sdk::CreditRole;
use serde::Serialize;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CreditMember {
    pub id: String,
    pub name: String,
    pub role: CreditRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
}

pub(crate) trait CreditRow {
    fn credit_recording_id(&self) -> Option<&str>;
    fn set_credit_members(&mut self, members: Vec<CreditMember>);
}

const MEMBERS: &str = "SELECT r.id, COALESCE(a.id, (SELECT rc.artist_id FROM account_recording_credits rc
            WHERE rc.user_id = ?1 AND rc.recording_id = r.id AND o.role = 'main')) AS artist_id,
        o.name, o.role, o.join_phrase
    FROM json_each(?2) wanted
    JOIN recordings r ON r.id = wanted.value
    JOIN library_members member ON member.library_id = r.library_id AND member.user_id = ?1
    JOIN recording_credited_artist_observations o ON o.observation_id = r.metadata_observation_id AND o.recording_id = r.id
    LEFT JOIN account_artists a ON a.user_id = ?1 AND a.library_id = r.library_id AND a.name = o.name COLLATE BINARY
    WHERE EXISTS (SELECT 1 FROM account_recording_credit_members m WHERE m.user_id = ?1 AND m.recording_id = r.id)
        AND artist_id IS NOT NULL
    ORDER BY r.id, o.role = 'featured', o.position";

pub(crate) async fn attach<T: CreditRow>(
    connection: &mut SqliteConnection,
    account: &str,
    rows: &mut [T],
) -> Result<()> {
    let wanted: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.credit_recording_id())
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let wanted = serde_json::to_string(&wanted)
        .map_err(|_| crate::Error::Internal("Recording selection could not be encoded".into()))?;
    let found: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(MEMBERS)
        .bind(account)
        .bind(wanted)
        .fetch_all(connection)
        .await?;
    if found.is_empty() {
        return Ok(());
    }
    let mut by_recording: HashMap<String, Vec<CreditMember>> = HashMap::new();
    for (recording, id, name, role, join) in found {
        by_recording
            .entry(recording)
            .or_default()
            .push(CreditMember {
                id,
                name,
                role: if role == "featured" {
                    CreditRole::Featured
                } else {
                    CreditRole::Main
                },
                join,
            });
    }
    for row in rows {
        let members = row
            .credit_recording_id()
            .and_then(|id| by_recording.remove(id));
        if let Some(members) = members {
            row.set_credit_members(members);
        }
    }
    tracing::debug!("Credit members attached");
    Ok(())
}
