use super::{ArtistAssociation, authority::Credit};
use crate::{Actor, Error, Result};
use hito_plugin_sdk::CreditRole;
use sqlx::SqliteConnection;

#[derive(Default, PartialEq, Eq, serde::Serialize)]
pub(super) enum CreditTarget {
    #[default]
    WholeCredit,
    NamedMember {
        artist_id: String,
        role: CreditRole,
    },
}

impl CreditTarget {
    pub(super) fn storage_kind(&self) -> &'static str {
        match self {
            Self::WholeCredit => "whole_credit",
            Self::NamedMember {
                role: CreditRole::Main,
                ..
            } => "main_member",
            Self::NamedMember {
                role: CreditRole::Featured,
                ..
            } => "featured_member",
        }
    }

    pub(super) async fn for_association(
        connection: &mut SqliteConnection,
        actor: &Actor,
        association: &ArtistAssociation,
    ) -> Result<Self> {
        let kind: String = sqlx::query_scalar(
            "SELECT credit_target FROM recording_artist_associations
             WHERE id=? AND user_id=? AND recording_id=?",
        )
        .bind(&association.id)
        .bind(actor.id())
        .bind(&association.recording_id)
        .fetch_optional(connection)
        .await?
        .ok_or(Error::NotFound)?;
        match kind.as_str() {
            "whole_credit" => Ok(Self::WholeCredit),
            "main_member" => Ok(Self::NamedMember {
                artist_id: association.artist_id.clone(),
                role: CreditRole::Main,
            }),
            "featured_member" => Ok(Self::NamedMember {
                artist_id: association.artist_id.clone(),
                role: CreditRole::Featured,
            }),
            _ => Err(Error::Internal(
                "Stored artist credit target is invalid".into(),
            )),
        }
    }

    pub(super) async fn capture_member(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
        mut credit: Credit,
        artist: &str,
        expected_role: Option<CreditRole>,
    ) -> Result<Credit> {
        let member: Option<(String, String)> = sqlx::query_as(
            "SELECT artist.name,member.role FROM account_recording_credit_members member
             JOIN account_artists artist ON artist.user_id=member.user_id
                AND artist.id=member.artist_id AND artist.library_id=?
             WHERE member.user_id=? AND member.recording_id=? AND member.artist_id=?",
        )
        .bind(&credit.library_id)
        .bind(actor.id())
        .bind(recording)
        .bind(artist)
        .fetch_optional(connection)
        .await?;
        let (name, role) = member.ok_or(super::authority::changed())?;
        let role = match role.as_str() {
            "main" => CreditRole::Main,
            "featured" => CreditRole::Featured,
            _ => {
                return Err(Error::Internal(
                    "Stored artist credit member is invalid".into(),
                ));
            }
        };
        if expected_role.is_some_and(|expected| expected != role) {
            return Err(super::authority::changed());
        }
        credit.artist_id = artist.into();
        credit.credited_name = name;
        credit.target = Self::NamedMember {
            artist_id: artist.into(),
            role,
        };
        Ok(credit)
    }
}
